import {Component, type ReactNode} from 'react';
import {createRoot} from 'react-dom/client';
import {ObjectView} from './components/ObjectView3D';
import type {RemoteScene} from './remote-session';
class SceneBoundary extends Component<{children:ReactNode},{failed:boolean}> {
  state={failed:false};
  static getDerivedStateFromError(){return {failed:true};}
  render(){return this.state.failed?<p>当前设备无法显示 3D，音频仍可正常播放。</p>:this.props.children;}
}
export function mountScene(element:HTMLElement){
 const root=createRoot(element);
 return {update(scene:RemoteScene,theme:'light'|'dark'){
   root.render(<SceneBoundary><ObjectView mobile objects={scene.objects} layout={scene.layout} theme={theme} mutedIds={new Set(scene.muted)} soundingIds={new Set(scene.sounding)} hiddenSpeakerNames={new Set(scene.hiddenSpeakers)}/></SceneBoundary>);
 },dispose(){root.unmount();}};
}
