param([Parameter(Mandatory=$true)][string]$DeviceId,[Parameter(Mandatory=$true)][ValidateRange(1,24)][int]$Channels,[uint32]$Mask=0)
$ErrorActionPreference='Stop'
# IPolicyConfig is a Windows compatibility interface, not a documented SDK
# contract. Verify the negotiated result and restore the old format on failure.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
[ComImport,Guid("870af99c-171d-4f9e-af0d-e63df40c2bc9")] class PolicyClient {}
[ComImport,Guid("f8679f50-850a-41cf-9c72-430f290290c8"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IPolicy {
 [PreserveSig] int GetMixFormat([MarshalAs(UnmanagedType.LPWStr)]string id,out IntPtr format);
 [PreserveSig] int GetDeviceFormat([MarshalAs(UnmanagedType.LPWStr)]string id,int ignored,out IntPtr format);
 [PreserveSig] int ResetDeviceFormat([MarshalAs(UnmanagedType.LPWStr)]string id);
 [PreserveSig] int SetDeviceFormat([MarshalAs(UnmanagedType.LPWStr)]string id,IntPtr device,IntPtr mix);

}
[StructLayout(LayoutKind.Sequential)] struct SdaKey { public Guid format; public uint pid; }
[StructLayout(LayoutKind.Explicit,Size=24)] struct SdaVariant { [FieldOffset(0)] public ushort type; [FieldOffset(8)] public uint value; }
[ComImport,Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class DeviceEnumerator {}
[ComImport,Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)] interface IDevices {
 [PreserveSig] int EnumAudioEndpoints(int flow,uint states,out object devices);
 [PreserveSig] int GetDefaultAudioEndpoint(int flow,int role,out IDevice device);
 [PreserveSig] int GetDevice([MarshalAs(UnmanagedType.LPWStr)]string id,out IDevice device);
}
[ComImport,Guid("D666063F-1587-4E43-81F1-B948E807363F"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)] interface IDevice {
 [PreserveSig] int Activate(ref Guid iid,uint context,IntPtr args,out object value);
 [PreserveSig] int OpenPropertyStore(uint mode,out IProperties properties);
}
[ComImport,Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)] interface IProperties {
 [PreserveSig] int GetCount(out uint count);
 [PreserveSig] int GetAt(uint index,out SdaKey key);
 [PreserveSig] int GetValue(ref SdaKey key,out SdaVariant value);
 [PreserveSig] int SetValue(ref SdaKey key,ref SdaVariant value);
 [PreserveSig] int Commit();
}
public static class SdaInputFormat {
 public static string Apply(string id,int channels,uint mask) {
  var policy=(IPolicy)new PolicyClient();
  var devices=(IDevices)new DeviceEnumerator();IDevice device;IProperties properties;
  Marshal.ThrowExceptionForHR(devices.GetDevice(id,out device));
  Marshal.ThrowExceptionForHR(device.OpenPropertyStore(2,out properties));
  IntPtr oldMix=IntPtr.Zero,oldDevice=IntPtr.Zero,next=IntPtr.Zero,deviceFormat=IntPtr.Zero,actual=IntPtr.Zero;
  bool changed=false;
  bool speakersChanged=false;
  var speakers=new SdaKey{format=new Guid("1da5d803-d492-4edd-8c23-e0c0ffee7f0e"),pid=3};
  SdaVariant oldSpeakers=new SdaVariant();
  try {
   Marshal.ThrowExceptionForHR(policy.GetMixFormat(id,out oldMix));
   Marshal.ThrowExceptionForHR(policy.GetDeviceFormat(id,0,out oldDevice));
   Marshal.ThrowExceptionForHR(properties.GetValue(ref speakers,out oldSpeakers));
   if(oldSpeakers.type!=19)throw new Exception("Invalid PhysicalSpeakers property type: "+oldSpeakers.type+" value: "+oldSpeakers.value+"");
   if(Marshal.ReadInt16(oldMix,2)==channels && (uint)Marshal.ReadInt32(oldMix,20)==mask && Marshal.ReadInt32(oldMix,4)==48000 && Marshal.ReadInt32(oldDevice,24)==1 && oldSpeakers.value==mask) return "unchanged";
   byte[] f=new byte[40];
   Action<int,ushort> u16=(at,value)=>Array.Copy(BitConverter.GetBytes(value),0,f,at,2);
   Action<int,uint> u32=(at,value)=>Array.Copy(BitConverter.GetBytes(value),0,f,at,4);
   u16(0,0xfffe);u16(2,(ushort)channels);u32(4,48000);u32(8,(uint)(48000*channels*4));u16(12,(ushort)(channels*4));u16(14,32);u16(16,22);u16(18,32);u32(20,mask);
   Array.Copy(new Guid("00000003-0000-0010-8000-00aa00389b71").ToByteArray(),0,f,24,16);
   next=Marshal.AllocCoTaskMem(40);Marshal.Copy(f,0,next,40);
   deviceFormat=Marshal.AllocCoTaskMem(40);f[24]=1;Marshal.Copy(f,0,deviceFormat,40);
   Marshal.ThrowExceptionForHR(policy.SetDeviceFormat(id,deviceFormat,next));changed=true;
   var desired=new SdaVariant{type=19,value=mask};
   Marshal.ThrowExceptionForHR(properties.SetValue(ref speakers,ref desired));speakersChanged=true;Marshal.ThrowExceptionForHR(properties.Commit());
   SdaVariant reported;
   Marshal.ThrowExceptionForHR(properties.GetValue(ref speakers,out reported));
   if(reported.type!=19 || reported.value!=mask)throw new Exception("Windows did not retain the speaker configuration");
   Marshal.ThrowExceptionForHR(policy.GetMixFormat(id,out actual));
   if(Marshal.ReadInt16(actual,2)!=channels || Marshal.ReadInt32(actual,4)!=48000 || (uint)Marshal.ReadInt32(actual,20)!=mask) throw new Exception("Windows did not retain the requested input layout");
   return "applied";
  } catch {
   if(speakersChanged){Marshal.ThrowExceptionForHR(properties.SetValue(ref speakers,ref oldSpeakers));Marshal.ThrowExceptionForHR(properties.Commit());}
   if(changed) Marshal.ThrowExceptionForHR(policy.SetDeviceFormat(id,oldDevice,oldMix));
   throw;
  } finally {
   foreach(var p in new[]{oldMix,oldDevice,next,deviceFormat,actual})if(p!=IntPtr.Zero)Marshal.FreeCoTaskMem(p);
   Marshal.ReleaseComObject(properties);Marshal.ReleaseComObject(device);Marshal.ReleaseComObject(devices);Marshal.ReleaseComObject(policy);
  }
 }
}
'@
# Only our experimental virtual endpoint may be changed by this helper.
$endpointKey=$DeviceId -replace '^\{0\.0\.0\.00000000\}\.',''
if($endpointKey -notmatch '^\{[0-9a-fA-F-]{36}\}$'){throw 'Invalid render endpoint ID'}
$properties=Get-ItemProperty -LiteralPath "HKLM:/SOFTWARE/Microsoft/Windows/CurrentVersion/MMDevices/Audio/Render/$endpointKey/Properties"
if(!($properties.PSObject.Properties.Value | Where-Object { $_ -is [string] -and $_ -match 'SDA (Spatial Bitstream|HDMI|Virtual HDMI)' })){throw 'Refusing to modify a non-SDA endpoint'}
[SdaInputFormat]::Apply($DeviceId,$Channels,$Mask)
