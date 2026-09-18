"""Reproduce the SDA experimental WaveRT driver from a pinned Microsoft sample.

Does not install a driver, change boot settings, or change the source checkout.
"""
from pathlib import Path
import argparse
import io
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
edit('adapter.cpp', 'gPCDriverUnloadRoutine = DriverObject->DriverUnload;',
     'ntStatus = SdaCaptureInitialize(DriverObject);\n    if (!NT_SUCCESS(ntStatus)) goto Done;\n    gPCDriverUnloadRoutine = DriverObject->DriverUnload;')
edit('adapter.cpp', 'if (gPCDriverUnloadRoutine != NULL)', 'SdaCaptureShutdown();\n    if (gPCDriverUnloadRoutine != NULL)')

file = 'EndpointsCommon/minwavertstream.cpp'
edit(file, '#include <limits.h>', '#include <limits.h>\n#include "SdaCapture.h"')
edit(file, 'm_KsState = State_;', 'if (!m_bCapture) SdaCaptureState(this, State_, &m_pWfExt->Format);\n    m_KsState = State_;')
edit(file, 'm_SaveData.WriteData(m_pDmaBuffer + bufferOffset, runWrite);',
     'SdaCaptureWrite(this, m_pDmaBuffer + bufferOffset, runWrite);')
edit(file, 'if (!g_DoNotCreateDataFiles)\n        {\n            // Read from buffer and write to a file.',
     'if (true)\n        {\n            // Transfer unchanged carrier bytes to the bounded receiver queue.')
edit(file, 'DPF_ENTER(("[CMiniportWaveRTStream::~CMiniportWaveRTStream]"));',
     'SdaCaptureClose(this);\n    DPF_ENTER(("[CMiniportWaveRTStream::~CMiniportWaveRTStream]"));')
edit(file, 'm_SaveData.Disable(drmRights->CopyProtect);',
     'm_SaveData.Disable(drmRights->CopyProtect);\n    SdaCaptureProtected(this, drmRights->CopyProtect || drmRights->DigitalOutputDisable);\n    if (drmRights->CopyProtect || drmRights->DigitalOutputDisable) return STATUS_ACCESS_DENIED;')
# If the timer missed an entire DMA buffer, those bytes were overwritten. Do
# not replay old ring contents as though they were newly produced audio.
edit(file, 'ULONG bufferOffset = m_ullLinearPosition % m_ulDmaBufferSize;\n\n    // Normally this will loop no more than once for a single wrap, but if\n    // many bytes have been displaced then this may loops many times.\n    while (ByteDisplacement > 0)\n    {\n        ULONG runWrite = min(ByteDisplacement, m_ulDmaBufferSize - bufferOffset);\n        SdaCaptureWrite',
     'if (ByteDisplacement > m_ulDmaBufferSize) {\n        SdaCaptureState(this, KSSTATE_PAUSE, &m_pWfExt->Format);\n        SdaCaptureState(this, KSSTATE_RUN, &m_pWfExt->Format);\n        return;\n    }\n    ULONG bufferOffset = m_ullLinearPosition % m_ulDmaBufferSize;\n    while (ByteDisplacement > 0)\n    {\n        ULONG runWrite = min(ByteDisplacement, m_ulDmaBufferSize - bufferOffset);\n        SdaCaptureWrite')

# A single output, no dummy microphones, Bluetooth takeover, or loopback tone.
edit('TabletAudioSample/minipairs.h', '    &SpeakerMiniports,\n    &SpeakerHpMiniports,\n    &HdmiMiniports,\n    &SpdifMiniports,', '    &HdmiMiniports,')
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

# Replace the sample's advertised codecs: only formats this prototype handles.
table = base / 'TabletAudioSample/hdmiwavtable.h'
text = table.read_text()
suffix = text[text.index('static\nPCPIN_DESCRIPTOR HdmiWaveMiniportPins[]'):]
prefix = '''// SDA experimental endpoint; remaining topology from Microsoft SysVAD.
#ifndef _SYSVAD_HDMIWAVTABLE_H_
#define _SYSVAD_HDMIWAVTABLE_H_
#define HDMI_DEVICE_MAX_CHANNELS 2
#define HDMI_MAX_INPUT_SYSTEM_STREAMS 1
#define HDMI_MAX_OUTPUT_LOOPBACK_STREAMS 0
'''
subtypes = ['PCM', 'IEC61937_DOLBY_DIGITAL_PLUS', 'IEC61937_DOLBY_DIGITAL_PLUS_ATMOS']
prefix += 'static KSDATAFORMAT_WAVEFORMATEXTENSIBLE HdmiHostPinSupportedDeviceFormats[] = {\n'
for i, sub in enumerate(subtypes):
    rate = 48000 if i == 0 else 192000
    mask = 'KSAUDIO_SPEAKER_STEREO' if i == 0 else 'KSAUDIO_SPEAKER_5POINT1'
    prefix += f'''{{{{sizeof(KSDATAFORMAT_WAVEFORMATEXTENSIBLE),0,0,0,
STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),STATICGUIDOF(KSDATAFORMAT_SUBTYPE_{sub}),STATICGUIDOF(KSDATAFORMAT_SPECIFIER_WAVEFORMATEX)}},
{{{{WAVE_FORMAT_EXTENSIBLE,2,{rate},{rate*4},4,16,22}},16,{mask},STATICGUIDOF(KSDATAFORMAT_SUBTYPE_{sub})}}}},
'''
prefix += '''};
static MODE_AND_DEFAULT_FORMAT HdmiHostPinSupportedDeviceModes[] = {{STATIC_AUDIO_SIGNALPROCESSINGMODE_RAW,NULL}};
static PIN_DEVICE_FORMATS_AND_MODES HdmiPinDeviceFormatsAndModes[] = {
{SystemRenderPin,HdmiHostPinSupportedDeviceFormats,SIZEOF_ARRAY(HdmiHostPinSupportedDeviceFormats),HdmiHostPinSupportedDeviceModes,1},
{RenderLoopbackPin,NULL,0,NULL,0},{BridgePin,NULL,0,NULL,0}};
static KSDATARANGE_AUDIO HdmiPinDataRangesStream[] = {
'''
for i, sub in enumerate(subtypes):
    rate = 48000 if i == 0 else 192000
    prefix += f'{{{{sizeof(KSDATARANGE_AUDIO),KSDATARANGE_ATTRIBUTES,0,0,STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),STATICGUIDOF(KSDATAFORMAT_SUBTYPE_{sub}),STATICGUIDOF(KSDATAFORMAT_SPECIFIER_WAVEFORMATEX)}},2,16,16,{rate},{rate}}},\n'
prefix += '};\nstatic PKSDATARANGE HdmiPinDataRangePointersStream[] = {\n'
for i in range(3):
    prefix += f'PKSDATARANGE(&HdmiPinDataRangesStream[{i}]),PKSDATARANGE(&PinDataRangeAttributeList),\n'
prefix += '''};
static PKSDATARANGE HdmiPinDataRangePointersLoopbackStream[] = {PKSDATARANGE(&HdmiPinDataRangesStream[0])};
static KSDATARANGE HdmiPinDataRangesBridge[] = {{sizeof(KSDATARANGE),0,0,0,STATICGUIDOF(KSDATAFORMAT_TYPE_AUDIO),STATICGUIDOF(KSDATAFORMAT_SUBTYPE_ANALOG),STATICGUIDOF(KSDATAFORMAT_SPECIFIER_NONE)}};
static PKSDATARANGE HdmiPinDataRangePointersBridge[] = {&HdmiPinDataRangesBridge[0]};
'''
table.write_text(prefix + suffix)
# Strictly bound the caller's extension before sample format comparison reads it.
edit('EndpointsCommon/minwavert.cpp', 'cPinFormats = GetPinSupportedDeviceFormats(_ulPin, &pPinFormats);',
     '''if (_pDataFormat->FormatSize < sizeof(KSDATAFORMAT_WAVEFORMATEX)) return STATUS_INVALID_PARAMETER;
    const auto wf = reinterpret_cast<PWAVEFORMATEX>(_pDataFormat + 1);
    if (ULONG(wf->cbSize) + sizeof(WAVEFORMATEX) > _pDataFormat->FormatSize - sizeof(KSDATAFORMAT)) return STATUS_INVALID_PARAMETER;
    if (wf->cbSize != 0 && wf->cbSize != 22 && wf->cbSize != 34) return STATUS_NO_MATCH;
    if (wf->cbSize == 34) {
        const auto encoded = reinterpret_cast<const ULONG*>(reinterpret_cast<const UCHAR*>(wf) + sizeof(WAVEFORMATEXTENSIBLE));
        if (encoded[0] != 48000 || encoded[1] != 6) return STATUS_NO_MATCH;
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
