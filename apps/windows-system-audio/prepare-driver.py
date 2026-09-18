"""Reproduce the SDA experimental WaveRT driver from a pinned Microsoft sample.

Does not install a driver, change boot settings, or change the source checkout.
"""
from pathlib import Path
import argparse
import io
import json
import re
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
PIN = '717778a20ba4dd2440fe609f69153a1f8a64f597'
parser = argparse.ArgumentParser()
parser.add_argument('--source', type=Path, default=ROOT / 'tmp/windows-driver-samples')
args = parser.parse_args()
if not args.source.exists():
    subprocess.run(['git', 'clone', '--filter=blob:none', '--no-checkout',
                    'https://github.com/microsoft/Windows-driver-samples.git', str(args.source)], check=True)
subprocess.run(['git', '-C', str(args.source), 'cat-file', '-e', PIN], check=True)
archive = subprocess.check_output(['git', '-C', str(args.source), 'archive', PIN, 'audio/sysvad', 'LICENSE'])
destination = ROOT / 'tmp/sda-system-audio-driver'
destination.mkdir(parents=True, exist_ok=True)
with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
    tar.extractall(destination, filter='data')
base = destination / 'audio/sysvad'

def edit(file, old, new):
    path = base / file
    text = path.read_text(encoding='utf-8-sig')
    if text.count(old) != 1:
        raise RuntimeError(f'{file}: expected exactly one patch anchor: {old[:70]}')
    path.write_text(text.replace(old, new), encoding='utf-8')

for name in ['SdaCapture.cpp', 'SdaCapture.h']:
    shutil.copyfile(Path(__file__).parent / 'driver' / name, base / name)

edit('adapter.cpp', '#include <sysvad.h>', '#include <sysvad.h>\n#include "SdaCapture.h"')
# Keep endpoint registration errors intact: the upstream sample overwrites them
# with optional interface queries and executes demonstration resource tests.
adapter_path = base / 'adapter.cpp'
adapter_text = adapter_path.read_text()
start = adapter_text.index('    PPORTCLSETWHELPER', adapter_text.index('InstallEndpointRenderFilters('))
end = adapter_text.index('    PAGED_CODE();', start)
adapter_text = adapter_text[:start] + adapter_text[end:]
start = adapter_text.index('    if (unknownWave) // IID_IPortClsEtwHelper')
end = adapter_text.index('    SAFE_RELEASE(unknownTopology);', start)
adapter_text = adapter_text[:start] + '''    SdaCaptureStartup(40, ntStatus);
    if (NT_SUCCESS(ntStatus) && unknownWave)
    {
        PPORTCLSETWHELPER helper = NULL;
        if (NT_SUCCESS(unknownWave->QueryInterface(IID_IPortClsEtwHelper, (PVOID*)&helper)))
        {
            _pAdapterCommon->SetEtwHelper(helper);
            helper->Release();
        }
    }

''' + adapter_text[end:]
adapter_path.write_text(adapter_text)
edit('adapter.cpp', 'DPF(D_TERSE, ("[DriverEntry]"));', 'DPF(D_TERSE, ("[DriverEntry]"));\n    SdaCaptureStartup(0, STATUS_SUCCESS);')
edit('adapter.cpp', 'return ntStatus;\n} // AddDevice', 'SdaCaptureStartup(10, ntStatus);\n    return ntStatus;\n} // AddDevice')
edit('common.cpp', '#include <sysvad.h>', '#include <sysvad.h>\n#include "SdaCapture.h"')
edit('common.cpp', 'ntStatus = CreateAudioInterfaceWithProperties(Name, TemplateName, cPropertyCount, pProperties, &symbolicLink);', 'ntStatus = CreateAudioInterfaceWithProperties(Name, TemplateName, cPropertyCount, pProperties, &symbolicLink);\n    SdaCaptureStartup(41, ntStatus);')
edit('common.cpp', 'ntStatus = PcNewPort(&port, PortClassId);', 'ntStatus = PcNewPort(&port, PortClassId);\n        SdaCaptureStartup(42, ntStatus);')
edit('common.cpp', '#pragma warning (pop)', '#pragma warning (pop)\n        SdaCaptureStartup(43, ntStatus);')
edit('common.cpp', 'ntStatus = PcGetPhysicalDeviceObject(DeviceObject, &m_pPhysicalDeviceObject);', 'ntStatus = PcGetPhysicalDeviceObject(DeviceObject, &m_pPhysicalDeviceObject);\n    SdaCaptureStartup(31, ntStatus);')
edit('common.cpp', '&m_WdfDevice);', '&m_WdfDevice);\n    SdaCaptureStartup(32, ntStatus);')
edit('adapter.cpp', 'if (gPCDriverUnloadRoutine != NULL)', 'SdaCaptureShutdown();\n    if (gPCDriverUnloadRoutine != NULL)')
for stage, anchor in enumerate([
    'ntStatus = pUnknownCommon->QueryInterface( IID_IAdapterCommon,(PVOID *) &pAdapterCommon);',
    'ntStatus = pAdapterCommon->Init(DeviceObject);',
    'ntStatus = PcRegisterAdapterPowerManagement( PUNKNOWN(pAdapterCommon), DeviceObject);',
    'ntStatus = InstallAllRenderFilters(DeviceObject, Irp, pAdapterCommon);',
    'ntStatus = InstallAllCaptureFilters(DeviceObject, Irp, pAdapterCommon);',
], 1):
    edit('adapter.cpp', anchor, anchor + f'\n    SdaCaptureStartup({stage}, ntStatus);')
edit('adapter.cpp', 'SdaCaptureStartup(5, ntStatus);\n    IF_FAILED_JUMP(ntStatus, Exit);',
     'SdaCaptureStartup(5, ntStatus);\n    IF_FAILED_JUMP(ntStatus, Exit);\n    ntStatus = SdaCaptureInitialize(DeviceObject->DriverObject);\n    SdaCaptureStartup(6, ntStatus);\n    IF_FAILED_JUMP(ntStatus, Exit);')
edit('adapter.cpp', 'case IRP_MN_REMOVE_DEVICE:\n    case IRP_MN_SURPRISE_REMOVAL:\n    case IRP_MN_STOP_DEVICE:',
     'case IRP_MN_REMOVE_DEVICE:\n    case IRP_MN_SURPRISE_REMOVAL:\n    case IRP_MN_STOP_DEVICE:\n        SdaCaptureShutdown();')

file = 'EndpointsCommon/minwavertstream.cpp'
edit(file, '#include <limits.h>', '#include <limits.h>\n#include "SdaCapture.h"')
edit('EndpointsCommon/minwavertstream.h', '    VOID WriteBytes', '    ULONGLONG m_SdaReturnCursor = 0;\n    VOID WriteBytes')
edit(file, 'm_ToneGenerator.GenerateSine(m_pDmaBuffer + bufferOffset, runWrite);', 'SdaReturnRead(&m_SdaReturnCursor, m_pDmaBuffer + bufferOffset, runWrite, &m_pWfExt->Format);')
edit(file, 'm_KsState = State_;', 'if (!m_bCapture) SdaCaptureState(this, State_, &m_pWfExt->Format);\n    else { SdaReturnState(m_KsState, State_); if (State_ == KSSTATE_RUN && m_KsState != KSSTATE_RUN) m_SdaReturnCursor = SdaReturnPosition(); }\n    m_KsState = State_;')
edit(file, 'm_SaveData.WriteData(m_pDmaBuffer + bufferOffset, runWrite);',
     'SdaCaptureWrite(this, m_pDmaBuffer + bufferOffset, runWrite);')
edit(file, 'if (!g_DoNotCreateDataFiles)\n        {\n            // Read from buffer and write to a file.',
     'if (true)\n        {\n            // Transfer unchanged carrier bytes to the bounded receiver queue.')
edit(file, 'DPF_ENTER(("[CMiniportWaveRTStream::~CMiniportWaveRTStream]"));',
     'if (m_bCapture) SdaReturnState(m_KsState, KSSTATE_STOP);\n    SdaCaptureClose(this);\n    DPF_ENTER(("[CMiniportWaveRTStream::~CMiniportWaveRTStream]"));')
edit(file, 'm_SaveData.Disable(drmRights->CopyProtect);',
     'm_SaveData.Disable(drmRights->CopyProtect);\n    SdaCaptureProtected(this, drmRights->CopyProtect || drmRights->DigitalOutputDisable);\n    if (drmRights->CopyProtect || drmRights->DigitalOutputDisable) return STATUS_ACCESS_DENIED;')
# If the timer missed an entire DMA buffer, those bytes were overwritten. Do
# not replay old ring contents as though they were newly produced audio.
edit(file, 'ULONG bufferOffset = m_ullLinearPosition % m_ulDmaBufferSize;\n\n    // Normally this will loop no more than once for a single wrap, but if\n    // many bytes have been displaced then this may loops many times.\n    while (ByteDisplacement > 0)\n    {\n        ULONG runWrite = min(ByteDisplacement, m_ulDmaBufferSize - bufferOffset);\n        SdaCaptureWrite',
     'if (ByteDisplacement > m_ulDmaBufferSize) {\n        SdaCaptureState(this, KSSTATE_PAUSE, &m_pWfExt->Format);\n        SdaCaptureState(this, KSSTATE_RUN, &m_pWfExt->Format);\n        return;\n    }\n    ULONG bufferOffset = m_ullLinearPosition % m_ulDmaBufferSize;\n    while (ByteDisplacement > 0)\n    {\n        ULONG runWrite = min(ByteDisplacement, m_ulDmaBufferSize - bufferOffset);\n        SdaCaptureWrite')

# A single output, no dummy microphones, Bluetooth takeover, or loopback tone.
# Hardware loopback returns the rendered stereo mix, never the input carrier.
edit('TabletAudioSample/minipairs.h', '    &SpeakerMiniports,\n    &SpeakerHpMiniports,\n    &HdmiMiniports,\n    &SpdifMiniports,', '    &HdmiMiniports,')
# Keep the system-default endpoint shared while a separate endpoint receives
# exclusive IEC61937. Both loopback pins read only SDA's rendered return ring.
pairs = base / 'TabletAudioSample/minipairs.h'
pair_text = pairs.read_text()
start = pair_text.index('static\nENDPOINT_MINIPAIR HdmiMiniports =')
end = pair_text.index('\n};', start) + 3
dedicated = pair_text[start:end].replace('HdmiMiniports =', 'SdaBitstreamMiniports =').replace('L"TopologyHdmi"', 'L"TopologySdaBitstream"').replace('L"WaveHdmi"', 'L"WaveSdaBitstream"')
pair_text = pair_text[:end] + '\n\n' + dedicated + pair_text[end:]
pair_text = pair_text.replace('    &HdmiMiniports,', '    &HdmiMiniports,\n    &SdaBitstreamMiniports,')
pairs.write_text(pair_text)
edit('TabletAudioSample/minipairs.h', '    &MicInMiniports,\n    &MicArray1Miniports,\n    &MicArray2Miniports,\n    &MicArray3Miniports,', '    nullptr,')
edit('TabletAudioSample/minipairs.h', '#define g_cCaptureEndpoints (SIZEOF_ARRAY(g_CaptureEndpoints))', '#define g_cCaptureEndpoints 0')
edit('adapter.cpp', '''    NTSTATUS            ntStatus;
    PENDPOINT_MINIPAIR* ppAeMiniports     = g_CaptureEndpoints;
\x20\x20\x20\x20
    PAGED_CODE();

    for(ULONG i = 0; i < g_cCaptureEndpoints; ++i, ++ppAeMiniports)
    {
        ntStatus = InstallEndpointCaptureFilters(_pDeviceObject, _pIrp, _pAdapterCommon, *ppAeMiniports);
        IF_FAILED_JUMP(ntStatus, Exit);
    }
\x20\x20\x20\x20
    ntStatus = STATUS_SUCCESS;

Exit:
    return ntStatus;''', '''    PAGED_CODE();
    UNREFERENCED_PARAMETER(_pDeviceObject);
    UNREFERENCED_PARAMETER(_pIrp);
    UNREFERENCED_PARAMETER(_pAdapterCommon);
    return STATUS_SUCCESS;''')

edit('TabletAudioSample/hdmitoptable.h', '    KSAUDIO_SPEAKER_STEREO,', '    0x2d63f, // 7.1.4; do not collapse shared system playback to stereo.')

# Replace the sample's advertised codecs: only formats this prototype handles.
table = base / 'TabletAudioSample/hdmiwavtable.h'
text = table.read_text()
suffix = text[text.index('static\nPCPIN_DESCRIPTOR HdmiWaveMiniportPins[]'):]
prefix = '''// SDA experimental endpoint; remaining topology from Microsoft SysVAD.
#ifndef _SYSVAD_HDMIWAVTABLE_H_
#define _SYSVAD_HDMIWAVTABLE_H_
#define HDMI_DEVICE_MAX_CHANNELS 24
#define HDMI_MAX_INPUT_SYSTEM_STREAMS 1
#define HDMI_MAX_OUTPUT_LOOPBACK_STREAMS 8
'''
formats = [('PCM', 12, 48000, 32, '0x2d63f')]
for channels, mask in [(1, '0x4'), (2, '0x3'), (6, '0x3f'), (6, '0x60f'), (8, '0x63f'), (12, '0x2d63f')]:
    for subtype, bits in [('PCM',16), ('PCM',24), ('PCM',32), ('IEEE_FLOAT',32)]:
        entry = (subtype, channels, 48000, bits, mask)
        if entry not in formats:
            formats.append(entry)
# Mask zero means discrete channels; the selected SDA input layout defines order.
layouts = json.loads((ROOT / 'apps/windows-system-audio/layouts.json').read_text())
for count in sorted({len(labels) for labels in layouts.values()}):
    for subtype, bits in [('PCM',16), ('PCM',24), ('PCM',32), ('IEEE_FLOAT',32)]:
        formats.append((subtype,count,48000,bits,'0'))
pcm_format_count = len(formats)
# The two-channel IEC carrier is not the decoded 5.1 speaker layout.
# Senders can describe the carrier as stereo/unspecified or use the content mask.
formats += [(sub, 2, 192000, 16, mask)
            for sub in ['IEC61937_DOLBY_DIGITAL_PLUS', 'IEC61937_DOLBY_DIGITAL_PLUS_ATMOS']
            for mask in ['0', 'KSAUDIO_SPEAKER_STEREO', 'KSAUDIO_SPEAKER_5POINT1', '0x60f', '0x63f']]
prefix += 'static KSDATAFORMAT_WAVEFORMATEXTENSIBLE HdmiHostPinSupportedDeviceFormats[] = {\n'
for sub, channels, rate, bits, mask in formats:
    block = channels * bits // 8
    prefix += f'''{{{{sizeof(KSDATAFORMAT_WAVEFORMATEXTENSIBLE),0,0,0,
STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),STATICGUIDOF(KSDATAFORMAT_SUBTYPE_{sub}),STATICGUIDOF(KSDATAFORMAT_SPECIFIER_WAVEFORMATEX)}},
{{{{WAVE_FORMAT_EXTENSIBLE,{channels},{rate},{rate*block},{block},{bits},22}},{bits},{mask},STATICGUIDOF(KSDATAFORMAT_SUBTYPE_{sub})}}}},
'''
prefix += '''};
static MODE_AND_DEFAULT_FORMAT HdmiHostPinSupportedDeviceModes[] = {{STATIC_AUDIO_SIGNALPROCESSINGMODE_RAW,NULL}};
static PIN_DEVICE_FORMATS_AND_MODES HdmiPinDeviceFormatsAndModes[] = {
{SystemRenderPin,HdmiHostPinSupportedDeviceFormats,SIZEOF_ARRAY(HdmiHostPinSupportedDeviceFormats),HdmiHostPinSupportedDeviceModes,1},
{RenderLoopbackPin,HdmiHostPinSupportedDeviceFormats,SDA_PCM_FORMAT_COUNT,NULL,0},{BridgePin,NULL,0,NULL,0}};
static KSDATARANGE_AUDIO HdmiPinDataRangesStream[] = {
'''
ranges = [('PCM',24,16,32,48000), ('IEEE_FLOAT',24,32,32,48000), ('IEC61937_DOLBY_DIGITAL_PLUS',2,16,16,192000), ('IEC61937_DOLBY_DIGITAL_PLUS_ATMOS',2,16,16,192000)]
for sub, channels, low, high, rate in ranges:
    prefix += f'{{{{sizeof(KSDATARANGE_AUDIO),KSDATARANGE_ATTRIBUTES,0,0,STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),STATICGUIDOF(KSDATAFORMAT_SUBTYPE_{sub}),STATICGUIDOF(KSDATAFORMAT_SPECIFIER_WAVEFORMATEX)}},{channels},{low},{high},{rate},{rate}}},\n'
prefix += '};\nstatic PKSDATARANGE HdmiPinDataRangePointersStream[] = {\n'
for i in range(len(ranges)):
    prefix += f'PKSDATARANGE(&HdmiPinDataRangesStream[{i}]),PKSDATARANGE(&PinDataRangeAttributeList),\n'
prefix += '''};
static KSDATARANGE_AUDIO HdmiLoopbackRanges[] = {
{{sizeof(KSDATARANGE_AUDIO),0,0,0,STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),STATICGUIDOF(KSDATAFORMAT_SUBTYPE_PCM),STATICGUIDOF(KSDATAFORMAT_SPECIFIER_WAVEFORMATEX)},24,16,32,48000,48000},
{{sizeof(KSDATARANGE_AUDIO),0,0,0,STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),STATICGUIDOF(KSDATAFORMAT_SUBTYPE_IEEE_FLOAT),STATICGUIDOF(KSDATAFORMAT_SPECIFIER_WAVEFORMATEX)},24,32,32,48000,48000}};
static PKSDATARANGE HdmiPinDataRangePointersLoopbackStream[] = {PKSDATARANGE(&HdmiLoopbackRanges[0]),PKSDATARANGE(&HdmiLoopbackRanges[1])};
static KSDATARANGE HdmiPinDataRangesBridge[] = {{sizeof(KSDATARANGE),0,0,0,STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),STATICGUIDOF(KSDATAFORMAT_SUBTYPE_ANALOG),STATICGUIDOF(KSDATAFORMAT_SPECIFIER_NONE)}};
static PKSDATARANGE HdmiPinDataRangePointersBridge[] = {&HdmiPinDataRangesBridge[0]};
'''
table.write_text(prefix.replace('SDA_PCM_FORMAT_COUNT', str(pcm_format_count)) + suffix)
# Strictly bound the caller's extension before sample format comparison reads it.
edit('EndpointsCommon/minwavert.cpp', 'cPinFormats = GetPinSupportedDeviceFormats(_ulPin, &pPinFormats);',
     '''if (_pDataFormat->FormatSize < sizeof(KSDATAFORMAT_WAVEFORMATEX)) return STATUS_INVALID_PARAMETER;
    const auto wf = reinterpret_cast<PWAVEFORMATEX>(_pDataFormat + 1);
    if (ULONG(wf->cbSize) + sizeof(WAVEFORMATEX) > _pDataFormat->FormatSize - sizeof(KSDATAFORMAT)) return STATUS_INVALID_PARAMETER;
    if (wf->cbSize != 0 && wf->cbSize != 22 && wf->cbSize != 34) return STATUS_NO_MATCH;
    if (wf->cbSize == 34) {
        const auto encoded = reinterpret_cast<const ULONG*>(reinterpret_cast<const UCHAR*>(wf) + sizeof(WAVEFORMATEXTENSIBLE));
        if (encoded[0] != 48000 || (encoded[1] != 2 && encoded[1] != 6 && encoded[1] != 8)) return STATUS_NO_MATCH;
    }
    if (wf->nAvgBytesPerSec != wf->nSamplesPerSec * wf->nBlockAlign) return STATUS_NO_MATCH;
    cPinFormats = GetPinSupportedDeviceFormats(_ulPin, &pPinFormats);''')

# Upstream has eight unguarded sideband branches even when its feature macros
# are disabled. Retain their ordinary-device else branches in this target.
node = base / 'EndpointsCommon/MiniportAudioEngineNode.cpp'
text = node.read_text()
removed = 0
while (start := text.find('    if (IsSidebandDevice() &&')) >= 0:
    opening = text.index('{', start)
    depth = 1
    end = opening + 1
    while depth:
        depth += (text[end] == '{') - (text[end] == '}')
        end += 1
    rest = text[end:]
    match = re.match(r'\s*else\s*', rest)
    text = text[:start] + '    ' + (rest[match.end():] if match else rest)
    removed += 1
if removed != 8:
    raise RuntimeError(f'Expected eight sideband branches, got {removed}')
node.write_text(text)

for relative in ['EndpointsCommon/EndpointsCommon.vcxproj', 'TabletAudioSample/TabletAudioSample.vcxproj']:
    project = base / relative
    text = project.read_text(encoding='utf-8-sig')
    text = text.replace('<KMDF_VERSION_MAJOR>1</KMDF_VERSION_MAJOR>', '<KMDF_VERSION_MAJOR>1</KMDF_VERSION_MAJOR><KMDF_VERSION_MINOR>33</KMDF_VERSION_MINOR>')
    text = text.replace(';SYSVAD_BTH_BYPASS', '').replace(';SYSVAD_USB_SIDEBAND', '')
    text = text.replace('<DriverTargetPlatform>Universal</DriverTargetPlatform>', '<DriverTargetPlatform>Desktop</DriverTargetPlatform>')
    if 'TabletAudioSample' in relative:
        text = text.replace('<TargetName>TabletAudioSample</TargetName>', '<TargetName>SdaSystemAudio</TargetName>')
        text = text.replace('$(DDK_LIB_PATH)\\libcntpr.lib', '$(DDK_LIB_PATH)\\libcntpr.lib;$(DDK_LIB_PATH)\\wdmsec.lib')
        text = re.sub(r'<Inf\s+[^>]+/>', '', text)
        text = text.replace('</Project>', '<ItemGroup><ClCompile Include="..\\SdaCapture.cpp" /></ItemGroup></Project>')
    project.write_text(text, encoding='utf-8')
shutil.copyfile(Path(__file__).parent / 'driver/SdaSystemAudio.inf', base / 'TabletAudioSample/SdaSystemAudio.inf')
print(base)
