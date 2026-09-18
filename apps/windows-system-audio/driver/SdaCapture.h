// SPDX-License-Identifier: Apache-2.0
#pragma once
#include <portcls.h>

// Buffered, bounded snapshots. Only SYSTEM/administrators may open the device.
#define SDA_READ CTL_CODE(FILE_DEVICE_UNKNOWN, 0x801, METHOD_BUFFERED, FILE_READ_DATA)
#define SDA_RETURN CTL_CODE(FILE_DEVICE_UNKNOWN, 0x802, METHOD_BUFFERED, FILE_WRITE_DATA)
struct SDA_CAPTURE_HEADER {
    ULONG magic;                 // "SDAC"
    ULONG version;
    ULONGLONG epoch;              // invalidates all previously received data
    ULONGLONG offset;             // first payload byte within this epoch
    ULONGLONG produced;
    ULONGLONG overflowCount;
    ULONG state;                 // KSSTATE
    ULONG payloadBytes;
    ULONG formatBytes;
    ULONG reserved;
    UCHAR format[64];             // WAVEFORMATEX + extension, not PCM samples
};
static_assert(sizeof(SDA_CAPTURE_HEADER) == 120, "capture ABI");
NTSTATUS SdaCaptureInitialize(PDRIVER_OBJECT driver);
VOID SdaCaptureShutdown();
VOID SdaCaptureState(PVOID stream, KSSTATE state, PWAVEFORMATEX format);
VOID SdaCaptureClose(PVOID stream);
VOID SdaCaptureWrite(PVOID stream, const UCHAR* bytes, ULONG count);
VOID SdaCaptureProtected(PVOID stream, BOOLEAN protectedContent);
VOID SdaCaptureStartup(ULONG stage, NTSTATUS status);
VOID SdaReturnState(KSSTATE previousState, KSSTATE state);
ULONGLONG SdaReturnPosition();
VOID SdaReturnRead(ULONGLONG* cursor, PUCHAR bytes, ULONG count, PWAVEFORMATEX format);
