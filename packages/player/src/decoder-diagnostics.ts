/** Audio-free boundary diagnostics. Never accepts byte arrays, text errors or paths. */
export class DecoderDiagnostics {
  private events:Record<string,unknown>[]=[];
  private sequence=0;
  private dropped=0;
  constructor(private enabled:()=>boolean,private send:(v:unknown)=>void){}
  reset(){this.events=[];this.sequence=0;this.dropped=0;}
  record(step:'open'|'packet'|'frame'|'flush'|'failure'|'checkpoint',values:Record<string,number|string|boolean>={}){
    if(!this.enabled())return;
    const fields:Record<string,unknown>={};
    // This list is intentionally closed. No arbitrary decoder state dumps.
    for(const key of ['bytes','units','sample','samples','rate','channels','objects','epoch','sequence','bit','declared','used','code','checkpointDropped']){
      const value=values[key];if(typeof value==='number'&&Number.isFinite(value))fields[key]=value;
    }
    if(typeof values.codec==='string'&&/^[a-z0-9_-]{1,24}$/i.test(values.codec))fields.codec=values.codec;
    if(typeof values.checkpoint==='string'&&/^[a-z0-9_.]{1,80}$/.test(values.checkpoint))fields.checkpoint=values.checkpoint;
    if(typeof values.errorTag==='string'&&/^[a-f0-9]{16}$/.test(values.errorTag))fields.errorTag=values.errorTag;
    for(const key of ['coreHash','framingHash'])if(typeof values[key]==='string'&&/^[a-f0-9]{64}$/.test(values[key] as string))fields[key]=values[key];
    const event={sequence:this.sequence++,time:performance.timeOrigin+performance.now(),step,...fields};
    this.events.push(event);if(this.events.length>96){this.events.shift();this.dropped++;}
    if(step==='failure')this.send({type:'decoderDiagnostic',schema:1,coverage:'decoder-checkpoints-v1',containsAudio:false,dropped:this.dropped,events:[...this.events]});
  }
}
