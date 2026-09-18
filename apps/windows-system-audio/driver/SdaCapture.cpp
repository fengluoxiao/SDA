// SPDX-License-Identifier: Apache-2.0
#include "SdaCapture.h"
#include <wdmsec.h>

// Persist startup failures even if PnP immediately unloads a failed device.
// Called only at PASSIVE_LEVEL from initialization, never the audio callback.
VOID SdaCaptureStartup(ULONG stage, NTSTATUS status) {
    UNICODE_STRING path = RTL_CONSTANT_STRING(L"\\Registry\\Machine\\System\\CurrentControlSet\\Services\\SdaSystemAudio\\Parameters");
    OBJECT_ATTRIBUTES attributes;
    InitializeObjectAttributes(&attributes, &path, OBJ_CASE_INSENSITIVE | OBJ_KERNEL_HANDLE, nullptr, nullptr);
    HANDLE key;
    if (NT_SUCCESS(ZwCreateKey(&key, KEY_SET_VALUE, &attributes, 0, nullptr, REG_OPTION_NON_VOLATILE, nullptr))) {
        UNICODE_STRING name = RTL_CONSTANT_STRING(L"StartupResult");
        ULONG values[] = {stage, ULONG(status)};
        ZwSetValueKey(key, &name, 0, REG_BINARY, values, sizeof(values));
        WCHAR stageName[] = L"Startup00";
        stageName[7] = WCHAR(L'0' + (stage / 10) % 10);
        stageName[8] = WCHAR(L'0' + stage % 10);
        RtlInitUnicodeString(&name, stageName);
        ZwSetValueKey(key, &name, 0, REG_DWORD, &status, sizeof(status));
        ZwClose(key);
    }
}

namespace {
constexpr ULONG Capacity = 2 * 1024 * 1024;
constexpr ULONG Tag = 'ACDS';
PDEVICE_OBJECT device;
struct ControlExtension { ULONG magic; };
constexpr ULONG ControlMagic = 0x43414453;
BOOLEAN dispatchInstalled;
PDRIVER_DISPATCH previous[IRP_MJ_MAXIMUM_FUNCTION + 1];
KSPIN_LOCK lock;
PUCHAR ring;
ULONG head, size;
PVOID source;
BOOLEAN connected, protectedSource;
SDA_CAPTURE_HEADER snapshot;
UNICODE_STRING link = RTL_CONSTANT_STRING(L"\\DosDevices\\SdaSystemAudio");

// Caller holds lock. Never splice bytes across a lost interval.
void Reset() {
    head = size = 0;
    ++snapshot.epoch;
    snapshot.offset = snapshot.produced = 0;
}
NTSTATUS Complete(PIRP irp, NTSTATUS status, ULONG_PTR bytes = 0) {
    irp->IoStatus.Status = status;
    irp->IoStatus.Information = bytes;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    return status;
}
NTSTATUS Dispatch(PDEVICE_OBJECT target, PIRP irp) {
    const auto stack = IoGetCurrentIrpStackLocation(irp);
    const auto major = stack->MajorFunction;
    if (target->DeviceType != FILE_DEVICE_UNKNOWN || !target->DeviceExtension ||
        static_cast<ControlExtension*>(target->DeviceExtension)->magic != ControlMagic)
        return previous[major](target, irp);
    // A removed control device may still have a user handle until Close. Keep
    // routing it here, never into PortCls with the wrong device extension.
    if (target != device)
        return Complete(irp, (major == IRP_MJ_CLOSE || major == IRP_MJ_CLEANUP) ? STATUS_SUCCESS : STATUS_DELETE_PENDING);
    KIRQL irql;
    if (major == IRP_MJ_CREATE) {
        KeAcquireSpinLock(&lock, &irql);
        if (!ring || target != device) { KeReleaseSpinLock(&lock, irql); return Complete(irp, STATUS_DELETE_PENDING); }
        const BOOLEAN busy = connected;
        if (!busy) { connected = TRUE; Reset(); }
        KeReleaseSpinLock(&lock, irql);
        return Complete(irp, busy ? STATUS_SHARING_VIOLATION : STATUS_SUCCESS);
    }
    if (major == IRP_MJ_CLEANUP) {
        KeAcquireSpinLock(&lock, &irql);
        if (target == device) { connected = FALSE; Reset(); }
        KeReleaseSpinLock(&lock, irql);
        return Complete(irp, STATUS_SUCCESS);
    }
    if (major == IRP_MJ_CLOSE) return Complete(irp, STATUS_SUCCESS);
    if (major != IRP_MJ_DEVICE_CONTROL || stack->Parameters.DeviceIoControl.IoControlCode != SDA_READ)
        return Complete(irp, STATUS_INVALID_DEVICE_REQUEST);
    const ULONG capacity = stack->Parameters.DeviceIoControl.OutputBufferLength;
    if (capacity < sizeof(SDA_CAPTURE_HEADER) || capacity > 65536)
        return Complete(irp, STATUS_INVALID_BUFFER_SIZE);
    auto output = static_cast<SDA_CAPTURE_HEADER*>(irp->AssociatedIrp.SystemBuffer);
    KeAcquireSpinLock(&lock, &irql);
    if (!ring || target != device) { KeReleaseSpinLock(&lock, irql); return Complete(irp, STATUS_DELETE_PENDING); }
    if (protectedSource) {
        KeReleaseSpinLock(&lock, irql);
        return Complete(irp, STATUS_ACCESS_DENIED);
    }
    const ULONG count = min(16384ul, min(size, capacity - sizeof(SDA_CAPTURE_HEADER)));
    snapshot.payloadBytes = count;
    *output = snapshot;
    auto payload = reinterpret_cast<PUCHAR>(output + 1);
    const ULONG first = min(count, Capacity - head);
    RtlCopyMemory(payload, ring + head, first);
    RtlCopyMemory(payload + first, ring, count - first);
    head = (head + count) % Capacity;
    size -= count;
    snapshot.offset += count;
    KeReleaseSpinLock(&lock, irql);
    return Complete(irp, STATUS_SUCCESS, sizeof(SDA_CAPTURE_HEADER) + count);
}
}

NTSTATUS SdaCaptureInitialize(PDRIVER_OBJECT driver) {
    if (device) return STATUS_SUCCESS;
    if (!dispatchInstalled) KeInitializeSpinLock(&lock);
    snapshot.magic = 0x43414453;
    snapshot.version = 1;
    ring = static_cast<PUCHAR>(ExAllocatePool2(POOL_FLAG_NON_PAGED, Capacity, Tag));
    if (!ring) return STATUS_INSUFFICIENT_RESOURCES;
    UNICODE_STRING name = RTL_CONSTANT_STRING(L"\\Device\\SdaSystemAudio");
    UNICODE_STRING security = RTL_CONSTANT_STRING(L"D:P(A;;GA;;;SY)(A;;GA;;;BA)");
    const GUID classId = {0x94e1680c,0xc45a,0x4c55,{0x91,0x17,0x2a,0x73,0xaf,0x69,0x2c,0x18}};
    NTSTATUS status = IoCreateDeviceSecure(driver, sizeof(ControlExtension), &name, FILE_DEVICE_UNKNOWN,
        FILE_DEVICE_SECURE_OPEN, TRUE, &security, &classId, &device);
    if (!NT_SUCCESS(status)) { ExFreePoolWithTag(ring, Tag); ring = nullptr; return status; }
    status = IoCreateSymbolicLink(&link, &name);
    if (!NT_SUCCESS(status)) { IoDeleteDevice(device); device = nullptr; ExFreePoolWithTag(ring, Tag); ring = nullptr; return status; }
    static_cast<ControlExtension*>(device->DeviceExtension)->magic = ControlMagic;
    if (!dispatchInstalled) {
        for (ULONG i = 0; i <= IRP_MJ_MAXIMUM_FUNCTION; ++i) {
            previous[i] = driver->MajorFunction[i];
            driver->MajorFunction[i] = Dispatch;
        }
        dispatchInstalled = TRUE;
    }
    device->Flags &= ~DO_DEVICE_INITIALIZING;
    return STATUS_SUCCESS;
}
VOID SdaCaptureShutdown() {
    if (!device && !ring) return;
    KIRQL irql;
    KeAcquireSpinLock(&lock, &irql);
    connected = FALSE;
    source = nullptr;
    snapshot.state = KSSTATE_STOP;
    Reset();
    PUCHAR retired = ring;
    ring = nullptr;
    KeReleaseSpinLock(&lock, irql);
    if (device) {
        IoDeleteSymbolicLink(&link);
        auto retiredDevice = device;
        device = nullptr;
        IoDeleteDevice(retiredDevice);
    }
    if (retired) ExFreePoolWithTag(retired, Tag);
}
VOID SdaCaptureState(PVOID stream, KSSTATE state, PWAVEFORMATEX format) {
    KIRQL irql;
    KeAcquireSpinLock(&lock, &irql);
    if (source != stream) {
        source = stream;
        protectedSource = FALSE;
        Reset();
    }
    // Pause/stop discard buffered audio, so the receiver never plays stale music.
    if (state != KSSTATE_RUN && ULONG(state) != snapshot.state) Reset();
    snapshot.state = state;
    snapshot.formatBytes = min(ULONG(sizeof(WAVEFORMATEX) + format->cbSize), ULONG(sizeof(snapshot.format)));
    RtlZeroMemory(snapshot.format, sizeof(snapshot.format));
    RtlCopyMemory(snapshot.format, format, snapshot.formatBytes);
    KeReleaseSpinLock(&lock, irql);
}
VOID SdaCaptureClose(PVOID stream) {
    KIRQL irql;
    KeAcquireSpinLock(&lock, &irql);
    if (source == stream) {
        source = nullptr;
        protectedSource = FALSE;
        snapshot.state = KSSTATE_STOP;
        snapshot.formatBytes = 0;
        Reset();
    }
    KeReleaseSpinLock(&lock, irql);
}
VOID SdaCaptureProtected(PVOID stream, BOOLEAN value) {
    KIRQL irql;
    KeAcquireSpinLock(&lock, &irql);
    if (source == stream) { protectedSource = value; Reset(); }
    KeReleaseSpinLock(&lock, irql);
}
VOID SdaCaptureWrite(PVOID stream, const UCHAR* bytes, ULONG count) {
    // Bound each spinlock hold. Called from the WaveRT DMA consumption path.
    while (count) {
        const ULONG chunk = min(count, 16384ul);
        KIRQL irql;
        KeAcquireSpinLock(&lock, &irql);
        if (ring && stream == source && connected && !protectedSource && snapshot.state == KSSTATE_RUN) {
            if (chunk > Capacity - size) { ++snapshot.overflowCount; Reset(); }
            const ULONG tail = (head + size) % Capacity;
            const ULONG first = min(chunk, Capacity - tail);
            RtlCopyMemory(ring + tail, bytes, first);
            RtlCopyMemory(ring, bytes + first, chunk - first);
            size += chunk;
            snapshot.produced += chunk;
        }
        KeReleaseSpinLock(&lock, irql);
        bytes += chunk;
        count -= chunk;
    }
}
