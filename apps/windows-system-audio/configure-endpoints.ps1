param([switch]$DiscoverOnly)
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
[ComImport,Guid("870af99c-171d-4f9e-af0d-e63df40c2bc9")] class PolicyClient {}
[ComImport,Guid("f8679f50-850a-41cf-9c72-430f290290c8"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)] interface IPolicy {
 int A(IntPtr a,IntPtr b); int B(IntPtr a,int b,IntPtr c); int C(IntPtr a); int D(IntPtr a,IntPtr b,IntPtr c);
 int E(IntPtr a,int b,IntPtr c,IntPtr d); int F(IntPtr a,IntPtr b); int G(IntPtr a,IntPtr b); int H(IntPtr a,IntPtr b);
 int I(IntPtr a,IntPtr b,IntPtr c); [PreserveSig] int SetPropertyValue([MarshalAs(UnmanagedType.LPWStr)]string id,int store,ref Key key,ref Value value);
 [PreserveSig] int SetDefaultEndpoint([MarshalAs(UnmanagedType.LPWStr)]string id,int role);
}
[StructLayout(LayoutKind.Sequential)] struct Key {public Guid format;public uint pid;}
[StructLayout(LayoutKind.Explicit,Size=24)] struct Value {[FieldOffset(0)]public ushort type;[FieldOffset(8)]public IntPtr pointer;}
[ComImport,Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class Enumerator {}
[ComImport,Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)] interface IDevices {
 int EnumAudioEndpoints(int flow,uint states,out object devices);
 [PreserveSig] int GetDefaultAudioEndpoint(int flow,int role,out IDevice device);
 [PreserveSig] int GetDevice([MarshalAs(UnmanagedType.LPWStr)]string id,out IDevice device);
}
[ComImport,Guid("D666063F-1587-4E43-81F1-B948E807363F"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)] interface IDevice {
 int Activate(IntPtr iid,uint context,IntPtr args,out object value);
 [PreserveSig] int OpenPropertyStore(uint mode,out IProperties properties);
 [PreserveSig] int GetId([MarshalAs(UnmanagedType.LPWStr)]out string id);
}
[ComImport,Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)] interface IProperties {
 int GetCount(out uint count);int GetAt(uint index,out Key key);int GetValue(ref Key key,out Value value);
 [PreserveSig] int SetValue(ref Key key,ref Value value);[PreserveSig] int Commit();
}
public static class SdaEndpointNames {
 public static void Rename(string id,string name) {
 var policy=(IPolicy)new PolicyClient();
 var value=new Value{type=31,pointer=Marshal.StringToCoTaskMemUni(name)};
 try {var key=new Key{format=new Guid("a45c254e-df1c-4efd-8020-67d146a850e0"),pid=2};
 Marshal.ThrowExceptionForHR(policy.SetPropertyValue(id,0,ref key,ref value));
 }finally{Marshal.FreeCoTaskMem(value.pointer);Marshal.ReleaseComObject(policy);}

 }
 public static void DefaultFromDedicated(string dedicated,string shared) {
 var devices=(IDevices)new Enumerator();var policy=(IPolicy)new PolicyClient();
 try{for(int role=0;role<2;role++){IDevice device;Marshal.ThrowExceptionForHR(devices.GetDefaultAudioEndpoint(0,role,out device));
 string id;try{Marshal.ThrowExceptionForHR(device.GetId(out id));}finally{Marshal.ReleaseComObject(device);}
 if(String.Equals(id,dedicated,StringComparison.OrdinalIgnoreCase))Marshal.ThrowExceptionForHR(policy.SetDefaultEndpoint(shared,role));
 }}finally{Marshal.ReleaseComObject(devices);Marshal.ReleaseComObject(policy);}
 }
}
'@
$shared=$null;$dedicated=$null
$knownSdaIds=@()
Get-ChildItem 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render' | ForEach-Object {
 $p=Get-ItemProperty -LiteralPath ($_.PSPath+'\Properties')
 $topology=$p.'{b3f8fa53-0004-438e-9003-51a46e139bfc},11'
 if($p.'{a8b865dd-2e3d-4094-ad97-e593a70c75d6},8' -ne 'Root\SdaSystemAudio'){return}
 $id='{0.0.0.00000000}.'+$_.PSChildName
 $script:knownSdaIds += $id
 # Updates leave stale endpoint records. Never pick one merely because its
 # friendly name/topology matches; DEVICE_STATE_ACTIVE is required.
 $state=(Get-ItemProperty -LiteralPath $_.PSPath).DeviceState
 if($state -ne 1){return}
 if($topology -match '\\topologysdabitstream$'){$script:dedicated=$id}
 elseif($topology -match '\\topologyhdmi$'){$script:shared=$id}
}
if(!$shared -or !$dedicated){throw 'SDA dual endpoints not found'}
if($DiscoverOnly){@{shared=$shared;dedicated=$dedicated}|ConvertTo-Json -Compress;exit}
try {[SdaEndpointNames]::Rename($shared,'SDA HDMI (System / Remote)')} catch {[Console]::Error.WriteLine($_)}
try {[SdaEndpointNames]::Rename($dedicated,'SDA HDMI (Dedicated)')} catch {[Console]::Error.WriteLine($_)}
[SdaEndpointNames]::DefaultFromDedicated($dedicated,$shared)
# Repair only explicit SDA selections in supported registry-backed players.
# Never replace a physical device or the user's generic default selection.
# A running player owns its in-memory settings, so defer instead of racing it.
$repairs=@();$pending=@()
foreach($player in @(@{key='PotPlayerMini64';process='PotPlayerMini64'},@{key='PotPlayerMini';process='PotPlayerMini'})) {
 $settings="HKCU:\Software\DAUM\$($player.key)\Settings"
 if(!(Test-Path -LiteralPath $settings)){continue}
 $values=Get-ItemProperty -LiteralPath $settings
 $selected=$values.IntAudioRenWSId
 if(!$selected -or $selected -eq $dedicated -or $selected -notin $knownSdaIds){continue}
 if($selected -eq $shared -and $values.IntAudioRenWSExclusive -ne 1){continue}
 if(Get-Process -Name $player.process -ErrorAction SilentlyContinue){$pending+=$player.key;continue}
 Set-ItemProperty -LiteralPath $settings -Name IntAudioRenWSId -Value $dedicated
 $repairs+=$player.key
}
@{shared=$shared;dedicated=$dedicated;repairedPlayers=$repairs;pendingPlayers=$pending}|ConvertTo-Json -Compress
