'use strict';
// System capture must terminate at a physical endpoint, never feed itself.
function sharedSystemOutput(status, requested, devices = []) {
  const selected = requested?.deviceId ? devices.find(d=>d.id===requested.deviceId) : null;
  const id = selected?.id ?? (requested?.deviceId || status?.actualId);
  const name = selected?.name ?? (id===status?.actualId ? status?.actualName : null);
  if (!id || !name || /SDA Spatial Bitstream/i.test(name)) {
    throw Error('请先选择实际耳机或音箱，不能将 SDA 虚拟输入作为输出');
  }
  if (id.startsWith('asio:') || id.startsWith('dsound:')) {
    throw Error('系统音频接收请先选择耳机或音箱的 WASAPI 输出，以支持共享播放和远程采集');
  }
  return {deviceId:id,exclusive:false,remoteCompatible:false};
}
module.exports = {sharedSystemOutput};
