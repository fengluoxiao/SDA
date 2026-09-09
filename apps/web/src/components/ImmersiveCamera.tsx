import { useEffect } from "react";
import { useThree } from "@react-three/fiber";
import { Euler, PerspectiveCamera } from "three";

// The scene origin is the acoustic listening position; future XR poses belong here.
export function ImmersiveCamera() {
  const { camera, gl, invalidate } = useThree();
  useEffect(() => {
    const position = camera.position.clone(), quaternion = camera.quaternion.clone();
    const zoom = camera.zoom, perspective = camera as PerspectiveCamera, fov = perspective.fov;
    camera.position.set(0, 0, 0);
    camera.quaternion.identity();
    camera.zoom = 1;
    perspective.fov = 85;
    camera.updateProjectionMatrix();
    invalidate();
    const canvas = gl.domElement;
    const previousTouchAction = canvas.style.touchAction;
    canvas.style.touchAction = "none";
    const rotation = new Euler(0, 0, 0, "YXZ");
    let pointer: number | null = null, x = 0, y = 0;
    const down = (event: PointerEvent) => {
      if (event.button !== 0 || pointer !== null) return;
      pointer = event.pointerId; x = event.clientX; y = event.clientY;
      canvas.setPointerCapture(pointer);
    };
    const move = (event: PointerEvent) => {
      if (event.pointerId !== pointer) return;
      rotation.y -= (event.clientX - x) * 0.004;
      rotation.x = Math.max(-Math.PI / 2 + 0.02, Math.min(Math.PI / 2 - 0.02, rotation.x - (event.clientY - y) * 0.004));
      x = event.clientX; y = event.clientY;
      camera.quaternion.setFromEuler(rotation);
      invalidate();
    };
    const up = (event: PointerEvent) => {
      if (event.pointerId !== pointer) return;
      if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
      pointer = null;
    };
    canvas.addEventListener("pointerdown", down);
    canvas.addEventListener("pointermove", move);
    canvas.addEventListener("pointerup", up);
    canvas.addEventListener("pointercancel", up);
    canvas.addEventListener("lostpointercapture", up);
    return () => {
      canvas.removeEventListener("pointerdown", down);
      canvas.removeEventListener("pointermove", move);
      canvas.removeEventListener("pointerup", up);
      canvas.removeEventListener("pointercancel", up);
      canvas.removeEventListener("lostpointercapture", up);
      if (pointer !== null && canvas.hasPointerCapture(pointer)) canvas.releasePointerCapture(pointer);
      canvas.style.touchAction = previousTouchAction;
      camera.position.copy(position); camera.quaternion.copy(quaternion);
      camera.zoom = zoom; perspective.fov = fov; camera.updateProjectionMatrix(); invalidate();
    };
  }, [camera, gl, invalidate]);
  return null;
}
