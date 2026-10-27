/** Raw ADM preservation is archival data, not an in-memory playback document. */
const fs = require('node:fs');
const CHUNK_LIMIT = 2 * 1024 * 1024 + 65536;
function scan(fd, start, bound = fs.fstatSync(fd).size) {
  let at = start, header = false, tail = false;
  const h = Buffer.alloc(21);
  for (;;) {
    if (at + 21 > bound || fs.readSync(fd, h, 0, 21, at) !== 21 || h[20] !== 10 || !/^[0-9a-f]{16}$/.test(h.toString('ascii', 4, 20))) throw Error('SDAR 母版归档截断');
    const id = h.toString('ascii', 0, 4), n = Number.parseInt(h.toString('ascii', 4, 20), 16);
    const end = at + 21 + n;
    if (!Number.isSafeInteger(end) || end > bound) throw Error('SDAR 母版归档超出文件');
    if (id === 'WVHC' && !tail && n >= 4 && n <= CHUNK_LIMIT) header = true;
    else if (id === 'WVTC' && header && n >= 4 && n <= CHUNK_LIMIT) tail = true;
    else if (id === 'WVEN' && header && n === 0) return {offset:start, bytes:end-start};
    else throw Error('SDAR 母版归档顺序或大小错误');
    at = end;
  }
}
function copyBox(input, output, archive, check = () => {}) {
  const at = fs.fstatSync(output).size, h = Buffer.alloc(16);
  h.writeUInt32BE(1);h.write('sdwe',4,'ascii');h.writeBigUInt64BE(BigInt(archive.bytes)+16n,8);
  fs.writeSync(output,h);
  const block=Buffer.alloc(64*1024);
  let copied=0;
  while(copied<archive.bytes){
    check();const n=Math.min(block.length,archive.bytes-copied);
    if(fs.readSync(input,block,0,n,archive.offset+copied)!==n)throw Error('SDAR 母版归档读取截断');
    let written=0;while(written<n){const done=fs.writeSync(output,block,written,n-written);if(!done)throw Error('SDAR 母版归档写入截断');written+=done;}
    copied+=n;
  }
  return at;
}
module.exports={scan,copyBox};
