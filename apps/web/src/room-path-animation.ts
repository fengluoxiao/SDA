export type Point3=[number,number,number];

export function pathPosition(points:Point3[],distance:number):Point3|null {
  if(distance<0||points.length<2)return null;
  for(let i=1;i<points.length;i++) {
    const a=points[i-1]!,b=points[i]!;
    const length=Math.hypot(b[0]-a[0],b[1]-a[1],b[2]-a[2]);
    if(length>0&&distance<=length) {
      const t=distance/length;
      return [a[0]+(b[0]-a[0])*t,a[1]+(b[1]-a[1])*t,a[2]+(b[2]-a[2])*t];
    }
    distance-=length;
  }
  return null;
}
