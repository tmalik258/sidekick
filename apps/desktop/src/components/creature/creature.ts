// @ts-nocheck
// biome-ignore-all lint: ported as is from the Mascot Lab prototype; keep the two in step.
// The Duskie creature: a small raymarched fur body (one shared WebGL 2 context draws every
// creature into its own 2D canvas, only when something changed) with the app's SVG eyes on top.
// Ported from the Mascot Lab prototype; keep the two in step.

const VERT = `#version 300 es
in vec2 aPos;
void main() { gl_Position = vec4(aPos, 0.0, 1.0); }`;

const FRAG = `#version 300 es
precision highp float;
out vec4 outColor;
uniform vec2 uRes;
uniform float uYaw, uPitch;
uniform vec3 uArmLA, uArmLM, uArmLB, uArmRA, uArmRM, uArmRB;
uniform vec3 uFur, uTip, uRim, uKey;
uniform float uRimI, uDim, uFurAmp, uDetail, uBump, uMicro, uTipK;
uniform vec3 uEyeCore, uEyeEdge;
uniform float uEmissive, uGlow;
uniform vec4 uEyeL, uEyeR;      // type, rx, ry, p (lid slope or arc radius)
uniform vec4 uEyeLB, uEyeRB;    // lid cut, extra, offset x, offset y
uniform vec2 uGaze;
uniform float uLid;
uniform vec4 uMouth;            // type, w, h, alpha
uniform float uBlush;
uniform float uZoom, uCamY;
uniform float uWig;
uniform vec3 uWigCol;
uniform float uGlint;

// ---- noise (Ashima / Gustavson simplex, MIT) ----
vec3 mod289(vec3 x){return x-floor(x*(1.0/289.0))*289.0;}
vec4 mod289(vec4 x){return x-floor(x*(1.0/289.0))*289.0;}
vec4 permute(vec4 x){return mod289(((x*34.0)+1.0)*x);}
vec4 taylorInvSqrt(vec4 r){return 1.79284291400159-0.85373472095314*r;}
float snoise(vec3 v){
  const vec2 C=vec2(1.0/6.0,1.0/3.0);
  const vec4 D=vec4(0.0,0.5,1.0,2.0);
  vec3 i=floor(v+dot(v,C.yyy));
  vec3 x0=v-i+dot(i,C.xxx);
  vec3 g=step(x0.yzx,x0.xyz);
  vec3 l=1.0-g;
  vec3 i1=min(g.xyz,l.zxy);
  vec3 i2=max(g.xyz,l.zxy);
  vec3 x1=x0-i1+C.xxx;
  vec3 x2=x0-i2+C.yyy;
  vec3 x3=x0-D.yyy;
  i=mod289(i);
  vec4 p=permute(permute(permute(i.z+vec4(0.0,i1.z,i2.z,1.0))+i.y+vec4(0.0,i1.y,i2.y,1.0))+i.x+vec4(0.0,i1.x,i2.x,1.0));
  float n_=0.142857142857;
  vec3 ns=n_*D.wyz-D.xzx;
  vec4 j=p-49.0*floor(p*ns.z*ns.z);
  vec4 x_=floor(j*ns.z);
  vec4 y_=floor(j-7.0*x_);
  vec4 x=x_*ns.x+ns.yyyy;
  vec4 y=y_*ns.x+ns.yyyy;
  vec4 h=1.0-abs(x)-abs(y);
  vec4 b0=vec4(x.xy,y.xy);
  vec4 b1=vec4(x.zw,y.zw);
  vec4 s0=floor(b0)*2.0+1.0;
  vec4 s1=floor(b1)*2.0+1.0;
  vec4 sh=-step(h,vec4(0.0));
  vec4 a0=b0.xzyw+s0.xzyw*sh.xxyy;
  vec4 a1=b1.xzyw+s1.xzyw*sh.zzww;
  vec3 p0=vec3(a0.xy,h.x);
  vec3 p1=vec3(a0.zw,h.y);
  vec3 p2=vec3(a1.xy,h.z);
  vec3 p3=vec3(a1.zw,h.w);
  vec4 norm=taylorInvSqrt(vec4(dot(p0,p0),dot(p1,p1),dot(p2,p2),dot(p3,p3)));
  p0*=norm.x;p1*=norm.y;p2*=norm.z;p3*=norm.w;
  vec4 m=max(0.6-vec4(dot(x0,x0),dot(x1,x1),dot(x2,x2),dot(x3,x3)),0.0);
  m=m*m;
  return 42.0*dot(m*m,vec4(dot(p0,x0),dot(p1,x1),dot(p2,x2),dot(p3,x3)));
}

// ---- shape ----
float sdRBox(vec3 p,vec3 b,float r){vec3 q=abs(p)-b+r;return length(max(q,0.0))+min(max(q.x,max(q.y,q.z)),0.0)-r;}
float smin(float a,float b,float k){float h=max(k-abs(a-b),0.0)/k;return min(a,b)-h*h*k*0.25;}
float sdEll(vec3 p,vec3 r){float k0=length(p/r);float k1=length(p/(r*r));return k0*(k0-1.0)/k1;}
float sdCone(vec3 p,vec3 a,vec3 b,float r1,float r2){
  vec3 pa=p-a,ba=b-a;float h=clamp(dot(pa,ba)/dot(ba,ba),0.0,1.0);
  return length(pa-ba*h)-mix(r1,r2,h);
}
// A soft plush arm: one even, round tube that bows gently through M (a curve, never a joint)
// and swells a little into a round paw at B.
vec3 bez(vec3 a,vec3 m,vec3 b,float t){float u=1.0-t;return u*u*a+2.0*u*t*m+t*t*b;}
float arm(vec3 p,vec3 a,vec3 m,vec3 b){
  // Distance to one smooth curved tube: find the nearest point on the curve (start from the
  // straight line, then two tangent steps), so the surface is seamless along its whole length.
  vec3 ab=b-a;
  float t=clamp(dot(p-a,ab)/dot(ab,ab),0.0,1.0);
  for(int i=0;i<2;i++){
    vec3 c=bez(a,m,b,t);
    vec3 tg=2.0*((1.0-t)*(m-a)+t*(b-m));
    t=clamp(t+dot(p-c,tg)/max(dot(tg,tg),1e-4),0.0,1.0);
  }
  float r=0.17+0.045*t*t*t;
  return length(p-bez(a,m,b,t))-r;
}
mat3 rot;
vec3 toObj(vec3 p){return rot*(p-vec3(0.0,-0.1,0.0))+vec3(0.0,-0.1,0.0);}
// The body is designed tall and drawn squashed: everything is 12% shorter.
const float HS=0.88;
vec3 design(vec3 p){return vec3(p.x,(p.y+0.86)/HS-0.86,p.z);}
// A dark hair cap with a wavy fringe over the forehead.
float wigMask(vec3 q){
  float edge=0.69+0.05*snoise(q*8.0)-0.03*smoothstep(0.15,0.5,q.z);
  // Hands raised into the hair zone stay furry, not haired.
  float ad=min(arm(q,uArmLA,uArmLM,uArmLB),arm(q,uArmRA,uArmRM,uArmRB));
  return smoothstep(edge-0.05,edge+0.04,q.y)*smoothstep(0.03,0.16,ad)*uWig;
}
float body(vec3 p){
  p=design(p);
  // Measured from the reference: an egg, narrower at the top, full at the bottom; a slimmer face.
  float taper=1.0+0.14*smoothstep(0.15,0.85,p.y);
  vec3 pt=vec3(p.x*taper,p.y,p.z);
  float egg=sdEll(pt-vec3(0.0,0.08,0.0),vec3(0.67,0.9,0.6));
  float dome=sdEll(pt-vec3(0.0,0.45,0.0),vec3(0.5,0.53,0.53));
  float low=sdEll(p-vec3(0.0,-0.36,0.03),vec3(0.67,0.48,0.59));
  float d=smin(smin(egg,dome,0.2),low,0.25);
  // An afro: a round, full cloud of tight coils sitting on the head.
  float afro=sdEll(p-vec3(0.0,0.88,-0.06),vec3(0.58,0.5,0.56));
  d=smin(d,afro,0.26);
  // Round bean feet peeking out in front.
  float fl=sdEll(p-vec3(-0.27,-0.87,0.13),vec3(0.2,0.11,0.2));
  float fr=sdEll(p-vec3(0.27,-0.87,0.13),vec3(0.2,0.11,0.2));
  d=smin(d,min(fl,fr),0.07);
  // Teardrop arms like the reference: thin where they leave the body, a round mitten at the end.
  float al=arm(p,uArmLA,uArmLM,uArmLB);
  float ar=arm(p,uArmRA,uArmRM,uArmRB);
  // Melt in at the shoulder, stay separate from the elbow down, so a hand can rest on the face.
  float kl=mix(0.16,0.012,clamp(length(p-uArmLA)/0.3,0.0,1.0));
  float kr=mix(0.16,0.012,clamp(length(p-uArmRA)/0.3,0.0,1.0));
  d=smin(d,al,kl);
  d=smin(d,ar,kr);
  d-=wigMask(p)*0.02;
  return d*HS;
}
float curls(vec3 p){
  vec3 q=p*uDetail;
  float grain=smoothstep(-0.15,0.8,snoise(q));
  float c=1.0-abs(snoise(q*2.3+vec3(7.0,1.3,4.1)));
  c=c*c*c*c;
  return clamp(grain*0.72+c*0.38,0.0,1.0);
}
float hair(vec3 q){float clump=smoothstep(-0.2,0.75,snoise(q*19.0));float a=1.0-abs(snoise(q*44.0+vec3(5.0)));a=a*a*a;return clamp(clump*0.7+a*0.45,0.0,1.0);}
float map(vec3 p){
  float d=body(p);
  if(d<0.08){
    vec3 q=design(p);
    float w=wigMask(q);
    d-=uFurAmp*curls(p)*(1.0-w)+w*0.06*hair(q);
  }
  return d*0.8;
}
vec3 normalAt(vec3 p){
  const vec2 k=vec2(1.0,-1.0);
  float e=0.0025;
  return normalize(k.xyy*map(p+k.xyy*e)+k.yyx*map(p+k.yyx*e)+k.yxy*map(p+k.yxy*e)+k.xxx*map(p+k.xxx*e));
}
vec3 baseNormal(vec3 p){
  const vec2 k=vec2(1.0,-1.0);
  float e=0.01;
  return normalize(k.xyy*body(p+k.xyy*e)+k.yyx*body(p+k.yyx*e)+k.yxy*body(p+k.yxy*e)+k.xxx*body(p+k.xxx*e));
}
float ao(vec3 p,vec3 n){
  float o=0.0,s=1.0;
  for(int i=1;i<=5;i++){float h=0.015+0.05*float(i);float d=body(p+n*h);o+=(h-d)*s;s*=0.7;}
  return clamp(1.0-2.4*o,0.0,1.0);
}

// ---- 2D face marks ----
float sdSeg(vec2 p,vec2 a,vec2 b){vec2 pa=p-a,ba=b-a;float h=clamp(dot(pa,ba)/dot(ba,ba),0.0,1.0);return length(pa-ba*h);}
float sdArc(vec2 p,float ap,float ra,float rb){vec2 sc=vec2(sin(ap),cos(ap));p.x=abs(p.x);return((sc.y*p.x>sc.x*p.y)?length(p-sc*ra):abs(length(p)-ra))-rb;}
float sdEll2(vec2 p,vec2 r){float k0=length(p/r);float k1=length(p/(r*r));return k0*(k0-1.0)/max(k1,1e-4);}
// type 0 ellipse (+ slanted top lid), 1 arc up (happy), 2 arc down (closed), 3 chevron, side = -1 left, 1 right
float eyeD(vec2 q,vec4 e,vec4 b,float side){
  float t=e.x;
  if(t>8.5) return 1.0;   // eyes drawn by the page on top (the app's own eyes)
  if(t<0.5){
    vec2 r=vec2(e.y,e.z*uLid);
    float d=sdEll2(q,r);
    float lid=b.x-e.w*q.x*side;      // top lid line, slanted toward the nose
    d=max(d,q.y-lid*uLid);
    return d;
  }
  if(t<1.5) return sdArc(q-vec2(0.0,-e.w*0.55),1.05,e.w,e.z);
  if(t<2.5) return sdArc(vec2(q.x,-q.y)-vec2(0.0,-e.w*0.75),0.85,e.w,e.z);
  float s=side;
  vec2 a=vec2(-0.045*s,0.05),c=vec2(0.04*s,0.0),d2=vec2(-0.045*s,-0.05);
  return min(sdSeg(q,a,c),sdSeg(q,c,d2))-e.z;
}
float mouthD(vec2 q){
  float t=uMouth.x,w=uMouth.y,h=uMouth.z;
  if(t<0.5) return 1.0;
  if(t<1.5) return sdArc(vec2(q.x,-q.y)-vec2(0.0,-w*0.9),0.9,w,h);        // smile
  if(t<2.5){float d=sdEll2(q,vec2(w,h));return max(d,q.y-h*0.25);}       // grin
  if(t<3.5 || t>6.5) return sdEll2(q,vec2(w,h));                                   // o / small
  if(t<4.5) return sdSeg(q,vec2(-w,0.0),vec2(w,0.0))-h;                   // flat
  if(t<5.5) return sdArc(q-vec2(0.0,-w*0.9),0.9,w,h);                     // frown
  vec2 r=vec2(abs(q.x)-w*0.5,q.y);                                        // cat
  return sdArc(vec2(r.x,-r.y)-vec2(0.0,-w*0.45),1.2,w*0.5,h);
}
// Face marks in object space (x, y) on the front of the head.
vec3 face(vec2 uv,out float mask,out float glowD){
  vec2 g=uGaze*vec2(0.05,0.035);
  vec2 cl=vec2(-0.18,0.19)+uEyeLB.zw+g;
  vec2 cr=vec2(0.18,0.19)+uEyeRB.zw+g;
  float dl=eyeD(uv-cl,uEyeL,uEyeLB,-1.0);
  float dr=eyeD(vec2(uv.x,uv.y)-cr,uEyeR,uEyeRB,1.0);
  float dm=mouthD(uv-vec2(0.0,-0.1)-g*0.6);
  float de=min(dl,dr);
  float d=min(de,uMouth.w>0.01?dm+(1.0-uMouth.w)*0.02:1.0);
  glowD=d;
  float aa=2.6*uZoom/uRes.y+0.002;
  mask=1.0-smoothstep(-aa,aa,d);
  if(uMouth.w<1.0 && dm<de) mask*=uMouth.w;
  float inner=clamp(-d/0.05,0.0,1.0);
  vec3 col=mix(uEyeEdge,uEyeCore,smoothstep(0.0,0.8,inner));
  // A yawning mouth is dark inside with a warm lip.
  if(uMouth.x>6.5 && dm<de) col=mix(uEyeEdge*0.9,uEyeEdge*0.12,smoothstep(0.1,0.55,clamp(-dm/0.04,0.0,1.0)));
  // A tiny glint on ink eyes.
  if(uGlint>0.5){
    float gl=length(uv-(cl+vec2(0.03,0.055)*uLid))-0.022;
    float gr=length(uv-(cr+vec2(0.03,0.055)*uLid))-0.022;
    col=mix(col,vec3(2.2),(1.0-smoothstep(-aa,aa,min(gl,gr)))*step(uEyeL.x,0.5));
  }
  return col;
}

void main(){
  vec2 frag=gl_FragCoord.xy;
  vec2 uv=(frag-0.5*uRes)/uRes.y;
  float cy=cos(uYaw),sy=sin(uYaw),cp=cos(uPitch),sp=sin(uPitch);
  mat3 ry=mat3(cy,0.0,-sy, 0.0,1.0,0.0, sy,0.0,cy);
  mat3 rx=mat3(1.0,0.0,0.0, 0.0,cp,sp, 0.0,-sp,cp);
  rot=rx*ry;
  vec3 ro=vec3(0.0,0.06+uCamY,5.4);
  vec3 rd=normalize(vec3(uv*2.0*0.205*uZoom,-1.0));
  rd=normalize(rd+vec3(0.0,-0.012,0.0));
  // Into object space.
  vec3 o=toObj(ro), dir=normalize(rot*rd);

  // Bounding sphere.
  vec3 bc=vec3(0.0,-0.05,0.0);
  float bR=1.35;
  vec3 oc=o-bc; float bb=dot(oc,dir); float cc=dot(oc,oc)-bR*bR; float disc=bb*bb-cc;
  vec4 col=vec4(0.0);
  float glowAcc=0.0;
  if(disc>0.0){
    float t=max(-bb-sqrt(disc),0.0), tEnd=-bb+sqrt(disc);
    bool hit=false; vec3 p;
    float minD=1e3; vec3 pc=o;
    for(int i=0;i<110;i++){
      p=o+dir*t;
      float d=body(p);
      if(d<0.08) d=map(p);
      if(d<minD){minD=d;pc=p;}
      if(d<0.0006*t){hit=true;break;}
      t+=max(d,0.0012);
      if(t>tEnd) break;
    }
    if(hit){
      vec3 n=normalAt(p);
      vec3 nb=baseNormal(p);
      vec3 nd=normalize(mix(nb,n,uBump));
      vec3 nr=normalize(mix(nb,n,0.25));
      float a=ao(p,nb);
      float tip=curls(p);
      vec3 v=-dir;
      // Lights live in world space; bring them into object space.
      vec3 kd=normalize(rot*vec3(-0.6,0.7,0.62));
      vec3 rd2=normalize(rot*vec3(0.75,0.35,-0.65));
      vec3 fd=normalize(rot*vec3(0.6,-0.2,0.8));
      float wrap=0.22;
      float dk=max((dot(nd,kd)+wrap)/(1.0+wrap),0.0);
      float df=max(dot(nd,fd),0.0);
      float fres=pow(1.0-max(dot(nr,v),0.0),2.5);
      float rimTerm=fres*(0.12+0.88*max(dot(nr,rd2)+0.3,0.0));
      vec3 h=normalize(kd+v);
      float sheen=pow(max(dot(n,h),0.0),12.0)*0.1*tip;
      vec3 alb=mix(uFur,uTip,pow(tip,2.0)*0.8*uTipK);
      vec3 qd=design(p);
      float wg=wigMask(qd);
      float strand=hair(qd);
      alb=mix(alb,mix(uWigCol*0.5,uWigCol*3.2,strand*strand),wg);
      float micro=mix(uMicro,1.0,tip);
      vec3 c=alb*(uKey*dk*(1.0-uDim*0.6)+vec3(0.07,0.075,0.085)*df+vec3(0.08,0.082,0.09)*(0.4+0.6*nb.y)+vec3(0.02))*a*micro;
      c+=uKey*sheen*a*(1.0-uDim*0.7);
      c+=wg*uKey*vec3(0.9,0.85,0.8)*0.2*pow(max(dot(nd,normalize(kd+v)),0.0),16.0)*strand*a;
      c+=alb*uKey*0.6*pow(1.0-max(dot(nr,v),0.0),1.25)*max(dot(nb,kd)+0.35,0.0)*a;
      c+=uRim*rimTerm*uRimI*(0.5+0.5*a);
      vec3 wd=normalize(rot*vec3(-0.85,0.25,-0.45));
      c+=uKey*vec3(1.0,0.82,0.62)*fres*max(dot(nr,wd)+0.15,0.0)*0.22*a;
      // Eye light spilling onto the fur around the eyes.
      if(uEmissive>0.5){
        vec3 el=vec3(-0.15,0.19,0.62), er=vec3(0.15,0.19,0.62);
        vec3 le=el-p, re=er-p;
        float sl=max(dot(n,normalize(le)),0.0)/(1.0+60.0*dot(le,le));
        float sr=max(dot(n,normalize(re)),0.0)/(1.0+60.0*dot(re,re));
        c+=uEyeEdge*(sl+sr)*uGlow*0.95*(0.35+tip);
      }
      // Blush.
      float bl=exp(-dot(p.xy-vec2(-0.25,0.03),p.xy-vec2(-0.25,0.03))*110.0)+exp(-dot(p.xy-vec2(0.25,0.03),p.xy-vec2(0.25,0.03))*110.0);
      c+=vec3(1.0,0.38,0.52)*bl*uBlush*0.35*step(0.0,p.z);
      // Face marks on the front.
      if(p.z>0.15){
        float m,gd;
        vec3 fc=face(p.xy,m,gd);
        float facing=smoothstep(0.15,0.45,p.z);
        m*=facing;
        vec3 ink=uEmissive>0.5?fc*1.25:fc;
        c=mix(c,ink,m);
      }
      col=vec4(c,1.0);
    }
    // Soft halo of fur at the silhouette: rays that pass close catch a little light.
    if(!hit){
      float fuzz=1.0-smoothstep(0.0,uFurAmp*2.6,minD);
      fuzz*=0.35+0.65*smoothstep(0.0,0.7,snoise(pc*uDetail*3.1));
      float fa=fuzz*0.8;
      col=vec4((uFur*0.6+uRim*uRimI*0.5)*fa,fa);
    }
  }
  // Glow from the eyes, measured on the plane of the face.
  if(uEmissive>0.5){
    float tp=(0.5-o.z)/dir.z;
    vec3 pp=o+dir*tp;
    float m,gd;
    face(pp.xy,m,gd);
    float glow=exp(-max(gd,0.0)*28.0)*0.42+exp(-max(gd,0.0)*10.0)*0.07*smoothstep(0.35,0.1,gd);
    glow*=uGlow;
    col.rgb+=uEyeEdge*glow;
    col.a=max(col.a,clamp(glow*0.9,0.0,1.0));
  }
  // Filmic tone, premultiplied out.
  vec3 c=col.rgb;
  c=c/(1.0+c*0.35);
  c=pow(c,vec3(1.0/1.0));
  float dz=fract(sin(dot(frag,vec2(12.9898,78.233)))*43758.5453)-0.5;
  outColor=vec4(c+dz/255.0,col.a+dz/255.0*step(0.002,col.a));
  outColor.rgb=min(outColor.rgb,vec3(outColor.a)+outColor.rgb*0.0);
}`;

class FurRenderer {
  constructor() {
    this.canvas = document.createElement("canvas");
    this.canvas.width = 640;
    this.canvas.height = 640;
    const gl = this.canvas.getContext("webgl2", {
      premultipliedAlpha: true,
      alpha: true,
      preserveDrawingBuffer: true,
      antialias: false,
    });
    if (!gl) throw new Error("WebGL2 unavailable");
    this.gl = gl;
    this.lost = false;
    this.canvas.addEventListener("webglcontextlost", (e) => {
      e.preventDefault();
      this.lost = true;
    });
    this.canvas.addEventListener("webglcontextrestored", () => {
      this.init();
      this.lost = false;
      this.onRestore?.();
    });
    this.init();
  }
  init() {
    const gl = this.gl;
    const sh = (type, src) => {
      const s = gl.createShader(type);
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
      return s;
    };
    const prog = gl.createProgram();
    gl.attachShader(prog, sh(gl.VERTEX_SHADER, VERT));
    gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FRAG));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog));
    gl.useProgram(prog);
    this.prog = prog;
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(prog, "aPos");
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    this.u = {};
    const n = gl.getProgramParameter(prog, gl.ACTIVE_UNIFORMS);
    for (let i = 0; i < n; i++) {
      const info = gl.getActiveUniform(prog, i);
      this.u[info.name] = gl.getUniformLocation(prog, info.name);
    }
  }

  /** Renders `params` at w x h pixels and copies it into `target` (a 2D canvas context). */
  draw(params, w, h, target) {
    const gl = this.gl;
    if (this.lost || gl.isContextLost()) return false;
    if (w > this.canvas.width || h > this.canvas.height) {
      this.canvas.width = Math.max(w, this.canvas.width);
      this.canvas.height = Math.max(h, this.canvas.height);
    }
    gl.viewport(0, 0, w, h);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    for (const [k, v] of Object.entries(params)) {
      const loc = this.u[k];
      if (!loc) continue;
      if (typeof v === "number") gl.uniform1f(loc, v);
      else if (v.length === 2) gl.uniform2fv(loc, v);
      else if (v.length === 3) gl.uniform3fv(loc, v);
      else gl.uniform4fv(loc, v);
    }
    gl.uniform2f(this.u.uRes, w, h);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    target.clearRect(0, 0, target.canvas.width, target.canvas.height);
    target.imageSmoothingEnabled = true;
    target.imageSmoothingQuality = "high";
    target.drawImage(this.canvas, 0, this.canvas.height - h, w, h, 0, 0, target.canvas.width, target.canvas.height);
    return true;
  }
}

let api = null;
let failed = false;

function boot() {
  let R;
  try {
    R = new FurRenderer();
  } catch {
    failed = true;
    return null;
  }
  // A software renderer draws the fur on the CPU and stalls the whole app; the orb is lighter.
  const info = R.gl.getExtension("WEBGL_debug_renderer_info");
  const gpu = String(info ? R.gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : R.gl.getParameter(R.gl.RENDERER));
  // Screenshot and test runs set this to draw on a software renderer.
  const forced = (window as unknown as { __creatureForce?: boolean }).__creatureForce === true;
  if (!forced && /swiftshader|llvmpipe|softpipe|basic render|microsoft basic/i.test(gpu)) {
    R.gl.getExtension("WEBGL_lose_context")?.loseContext();
    failed = true;
    return null;
  }
  R.onRestore = () => {
    for (const m of all) m.dirty = true;
  };
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const lin = (h) => [1, 3, 5].map((i) => (parseInt(h.slice(i, i + 2), 16) / 255) ** 2.2);
  const rnd = (a, b) => a + Math.random() * (b - a);

  // ---------- Fur colours ----------
  const THEMES = {
    atelier: {
      label: "Atelier",
      fur: "#3b3b3d",
      tip: "#7c7b78",
      rim: "#b7bccb",
      rimI: 0.9,
      core: [1.0, 0.96, 0.84],
      edge: [0.98, 0.78, 0.5],
      emissive: 1,
      bump: 0.4,
      micro: 0.5,
      sw: ["#4a4c4b", "#0c0d0d"],
    },
    onyx: {
      label: "Onyx",
      fur: "#222227",
      tip: "#62656f",
      rim: "#a4b4dc",
      rimI: 0.95,
      core: [0.96, 0.98, 1.0],
      edge: [0.6, 0.76, 1.0],
      emissive: 1,
      bump: 0.4,
      micro: 0.5,
      sw: ["#3a3a44", "#050506"],
    },
    pearl: {
      label: "Pearl",
      fur: "#c6c9d3",
      tip: "#ffffff",
      rim: "#bde0fe",
      rimI: 0.45,
      core: lin("#1c1c1e"),
      edge: lin("#1c1c1e"),
      emissive: 0,
      bump: 0.3,
      micro: 0.78,
      sw: ["#ffffff", "#a9adb9"],
    },
    aurora: {
      label: "Aurora",
      fur: "#c9c1ea",
      tip: "#ffffff",
      rim: "#ffb3d9",
      rimI: 0.5,
      core: lin("#1b1630"),
      edge: lin("#1b1630"),
      emissive: 0,
      bump: 0.3,
      micro: 0.78,
      sw: ["#ffffff", "#a99fd6"],
    },
    chrome: {
      label: "Chrome",
      fur: "#9fa5b2",
      tip: "#f6f8fb",
      rim: "#c9d2e3",
      rimI: 0.5,
      core: lin("#0f1116"),
      edge: lin("#0f1116"),
      emissive: 0,
      bump: 0.35,
      micro: 0.7,
      sw: ["#f4f6fa", "#6f7584"],
    },
    peach: {
      label: "Peach",
      fur: "#f0a3a1",
      tip: "#fff3ea",
      rim: "#ffd6a5",
      rimI: 0.45,
      core: lin("#3a1716"),
      edge: lin("#3a1716"),
      emissive: 0,
      bump: 0.3,
      micro: 0.78,
      sw: ["#fff3ea", "#e8767c"],
    },
    mint: {
      label: "Mint",
      fur: "#92d6c4",
      tip: "#f2fffb",
      rim: "#b9f6ff",
      rimI: 0.45,
      core: lin("#0e2a23"),
      edge: lin("#0e2a23"),
      emissive: 0,
      bump: 0.3,
      micro: 0.78,
      sw: ["#f2fffb", "#4eb29b"],
    },
    lilac: {
      label: "Lilac",
      fur: "#b7a6ee",
      tip: "#fbf8ff",
      rim: "#ffc8f0",
      rimI: 0.45,
      core: lin("#1d1440"),
      edge: lin("#1d1440"),
      emissive: 0,
      bump: 0.3,
      micro: 0.78,
      sw: ["#fbf8ff", "#8a71df"],
    },
  };

  // ---------- Faces (shader units) ----------
  // The app's eye shapes, exactly as in the current orb (20 x 30 box).
  const APP_EYES = {
    pill: '<rect x="5" y="3" width="10" height="24" rx="5"/>',
    wide: '<rect x="3.5" y="1" width="13" height="28" rx="6.5"/><circle class="gl" cx="12.4" cy="7.2" r="2.2"/>',
    big: '<rect x="4" y="2" width="12" height="26" rx="6"/>',
    small: '<rect x="6" y="9" width="8" height="15" rx="4"/>',
    half: '<rect x="5" y="14" width="10" height="11" rx="5"/>',
    focus: '<rect x="4" y="11" width="12" height="9" rx="4.5"/>',
    closed: '<path class="ln" d="M4 16 Q10 19 16 16" stroke-width="3"/>',
    happy: '<path class="ln" d="M4 18.5 Q10 8.5 16 18.5" stroke-width="3.4"/>',
    dot: '<circle cx="10" cy="16" r="3.8"/>',
    squintL: '<path class="ln" d="M5.5 10 L14.5 15 L5.5 20" stroke-width="3.2"/>',
    squintR: '<path class="ln" d="M14.5 10 L5.5 15 L14.5 20" stroke-width="3.2"/>',
    worriedL: '<path d="M5 12 L15 8.5 L15 23 a5 5 0 0 1 -10 0 Z"/>',
    worriedR: '<path d="M15 12 L5 8.5 L5 23 a5 5 0 0 0 10 0 Z"/>',
    sleepyL: '<path d="M5 17.6 L15 15.4 L15 21 a5 5 0 0 1 -10 0 Z"/>',
    sleepyR: '<path d="M5 15.4 L15 17.6 L15 21 a5 5 0 0 1 -10 0 Z"/>',
    yawnL: '<path class="ln" d="M3.5 17 Q10 12.5 16.5 15.5" stroke-width="3.4"/>',
    yawnR: '<path class="ln" d="M3.5 15.5 Q10 12.5 16.5 17" stroke-width="3.4"/>',
  };
  // Eye ink and glow per fur, as in the app's themes.
  const EYE_INK = {
    atelier: ["#fff3dc", "255 206 140", 0.95],
    onyx: ["#ffffff", "205 225 255", 0.9],
    pearl: ["#1c1c1e", "0 0 0", 0],
    aurora: ["#1b1630", "0 0 0", 0],
    chrome: ["#0f1116", "0 0 0", 0],
    peach: ["#3a1716", "0 0 0", 0],
    mint: ["#0e2a23", "0 0 0", 0],
    lilac: ["#1d1440", "0 0 0", 0],
  };
  const E = {
    pill: [0, 0.064, 0.108, 0],
    wide: [0, 0.075, 0.124, 0],
    big: [0, 0.07, 0.116, 0],
    small: [0, 0.048, 0.08, 0],
    half: [0, 0.066, 0.104, 0],
    focus: [0, 0.07, 0.045, 0],
    dot: [0, 0.045, 0.05, 0],
    worriedL: [0, 0.064, 0.108, 0.55],
    worriedR: [0, 0.064, 0.108, 0.55],
    happy: [1, 0, 0.025, 0.085],
    closed: [2, 0, 0.022, 0.095],
    squintL: [3, 0, 0.02, 0],
    squintR: [3, 0, 0.02, 0],
  };
  const LID = { half: 0.012, worriedL: 0.05, worriedR: 0.05 };
  const M = {
    none: [0, 0, 0],
    smile: [1, 0.08, 0.02],
    grin: [2, 0.09, 0.07],
    o: [3, 0.042, 0.052],
    small: [3, 0.034, 0.024],
    flat: [4, 0.055, 0.018],
    frown: [5, 0.075, 0.02],
    cat: [6, 0.1, 0.018],
    talk: [3, 0.07, 0.085],
  };
  const AI = "#d6a8ff";
  // Arm poses: [left, right], each a point the paw reaches toward in the creature's own space,
  // [x, y, z], with x pointing away from the body on that side and z toward you.
  // Only the direction matters: the arm points from the shoulder toward that point.
  const REST = [0.9, -0.48, 0.14];
  const both = (a) => [a, a];
  const ARMS = {
    rest: both(REST),
    out: both([1.0, -0.06, 0.3]),
    in: both([0.4, -0.3, 0.75]),
    up: both([0.84, 0.66, 0.2]),
    ear: [REST, [0.66, 0.6, 0.18]],
    chin: [REST, [0.12, -0.08, 0.9]],
    wave: [REST, [0.92, 0.56, 0.24]],
    shy: both([0.3, -0.16, 0.9]),
    pillow: [
      [0.12, 0.3, 0.9],
      [-0.02, 0.24, 0.9],
    ],
  };
  // Expressions follow the Mascot Lab: a mouth only on reactions, the resting moods keep a quiet face.
  const X = {
    hm: {
      label: "Hm?",
      when: "You hover over it",
      eyes: ["big", "pill"],
      pose: { tilt: 10 },
      accent: "lift",
      effect: "hm",
    },
    idle: { label: "Idle", when: "Resting at the top", eyes: "pill" },
    suggest: { label: "Suggest", when: "Has an idea", eyes: "pill", rim: "#7fc0ff", accent: "lift", arms: "out" },
    curious: {
      label: "Notice",
      when: "Something changed",
      eyes: ["big", "pill"],
      rim: "#ffbf6b",
      pose: { tilt: 8 },
      accent: "lift",
      arms: "in",
    },
    listen: {
      label: "Listen",
      when: "Wake word or Ctrl+Alt+Space",
      eyes: "wide",
      rim: AI,
      pose: { scale: 1.03, tilt: -8 },
      arms: "ear",
      life: "pulse",
    },
    think: {
      label: "Think",
      when: "Working on an answer",
      eyes: "pill",
      gaze: [-0.45, -0.65],
      rim: AI,
      effect: "dots",
      arms: "chin",
      life: "sway",
    },
    speak: { label: "Speak", when: "Reading an answer aloud", eyes: "big", mouth: "talk", rim: AI, life: "talk" },
    happy: {
      label: "Happy",
      when: "Task done",
      eyes: "happy",
      mouth: "smile",
      blush: true,
      rim: "#8fe3a8",
      accent: "hop",
    },
    delight: {
      label: "Delighted",
      when: "A big task finished",
      eyes: "happy",
      mouth: "grin",
      blush: true,
      rim: "#ffd36b",
      accent: "hop2",
      effect: "glints",
    },
    hello: {
      label: "Hello",
      when: "First launch, morning",
      eyes: "happy",
      mouth: "smile",
      blush: true,
      pose: { tilt: -4 },
      accent: "hello",
      arms: "wave",
    },
    celebrate: {
      label: "Celebrate",
      when: "PR merged, all green",
      eyes: "happy",
      mouth: "grin",
      blush: true,
      rim: "#ffd36b",
      accent: "hop2",
      effect: "glints",
      arms: "up",
    },
    love: {
      label: "Love",
      when: "You said thanks",
      eyes: "happy",
      mouth: "cat",
      blush: true,
      rim: "#ff9cc0",
      pose: { tilt: -5 },
      accent: "nod",
      effect: "heart",
      arms: "in",
    },
    wink: {
      label: "Wink",
      when: "Saved you a few clicks",
      eyes: "pill",
      mouth: "smile",
      pose: { tilt: -6 },
      accent: "wink",
    },
    proud: {
      label: "Proud",
      when: "Your week looks good",
      eyes: "happy",
      mouth: "cat",
      gaze: [0, -0.4],
      rim: "#ffe9a8",
      pose: { y: -0.02, tilt: -3 },
      accent: "lift",
      effect: "glints",
    },
    surprised: {
      label: "Surprised",
      when: "Meeting in 2 minutes",
      eyes: "wide",
      mouth: "o",
      rim: "#ffbf6b",
      pose: { y: -0.015, scale: 1.03 },
      accent: "jump",
      arms: "out",
    },
    shy: {
      label: "Shy",
      when: "Compliment",
      eyes: "dot",
      gaze: [0.35, 0.7],
      mouth: "small",
      blush: true,
      pose: { tilt: 7, scale: 0.95, y: 0.015 },
      accent: "sink",
      arms: "shy",
    },
    focus: { label: "Focus", when: "Focus mode on", eyes: "focus", rim: "#7f9cff", life: "still" },
    worried: {
      label: "Worried",
      when: "Low battery, usage limit",
      eyes: ["worriedL", "worriedR"],
      mouth: "flat",
      rim: "#ffa36b",
      pose: { scale: 0.97 },
      accent: "shiver",
      arms: "in",
    },
    sad: {
      label: "Sad",
      when: "Failed twice in a row",
      eyes: ["worriedL", "worriedR"],
      gaze: [0, 0.6],
      mouth: "frown",
      rim: "#8a88ff",
      pose: { y: 0.03, scale: 0.95 },
      accent: "sink",
      arms: "in",
      glow: 0.7,
    },
    oops: {
      label: "Oops",
      when: "Something failed",
      eyes: ["squintL", "squintR"],
      mouth: "flat",
      rim: "#ff7a6b",
      pose: { scale: 0.97 },
      accent: "shiver",
      arms: "out",
    },
    tired: {
      label: "Tired",
      when: "Ten hours at it",
      eyes: ["sleepyL", "sleepyR"],
      mouth: "flat",
      pose: { y: 0.02, scale: 0.97, tilt: -3 },
      accent: "sink",
      glow: 0.6,
      life: "nodOff",
    },
    sleepy: {
      label: "Sleepy",
      when: "1 AM, fighting to stay up",
      eyes: "pill",
      pose: { y: 0.015 },
      glow: 0.7,
      life: "fightSleep",
    },
    sleep: {
      label: "Sleep",
      when: "Paused or away",
      eyes: "closed",
      dim: 0.6,
      pose: { y: 0.02, scale: 0.97, tilt: -12 },
      effect: "zzz",
      arms: "pillow",
      glow: 0.25,
      life: "slow",
    },
    offline: {
      label: "Offline",
      when: "No connection",
      eyes: "small",
      gaze: [0, 0.25],
      mouth: "flat",
      rim: "#9a9aa0",
      dim: 0.5,
      pose: { y: 0.025, scale: 0.96 },
      accent: "sink",
      effect: "nowifi",
      glow: 0.55,
    },
  };

  // The app plays its own sounds; the creature stays quiet.
  const Sound = { play() {} };
  const XS = {};

  class Spring {
    constructor(v, response, zeta) {
      this.value = v;
      this.target = v;
      this.vel = 0;
      this.k = ((2 * Math.PI) / response) ** 2;
      this.c = (4 * Math.PI * zeta) / response;
    }
    to(t) {
      this.target = t;
    }
    kick(v) {
      this.vel += v;
    }
    step(dt) {
      const a = -this.k * (this.value - this.target) - this.c * this.vel;
      this.vel += a * dt;
      this.value += this.vel * dt;
    }
    get resting() {
      return Math.abs(this.vel) < 1e-3 && Math.abs(this.value - this.target) < 1e-3;
    }
    snap() {
      this.value = this.target;
      this.vel = 0;
    }
  }

  // One straight stubby arm, like Doraemon's: sewn on at the middle of the side, just below the eyes,
  // it swings as a whole toward the paw target and never bends.
  const ARM_LEN = 0.6;
  function armSeg(side, h) {
    const S = [0.54 * side, 0.2, 0.08];
    const d = [h[0] * side - S[0], h[1] - S[1], h[2] - S[2]];
    const n = Math.hypot(...d) || 1e-4;
    const u = d.map((v) => v / n);
    const B = S.map((v, i) => v + u[i] * ARM_LEN);
    // Bow the middle outward and a touch down, so the arm curves like a stuffed limb.
    let o = [side, -0.35, 0];
    const k = o[0] * u[0] + o[1] * u[1] + o[2] * u[2];
    o = o.map((v, i) => v - k * u[i]);
    const ol = Math.hypot(...o) || 1;
    const M = S.map((v, i) => (v + B[i]) / 2 + (o[i] / ol) * 0.09);
    return [S, M, B];
  }
  const mixHex = (a, b, t) => {
    const A = lin(a),
      B = lin(b);
    return A.map((v, i) => v + (B[i] - v) * t);
  };

  const all = [];
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  let lastPointer = 0;

  class Mascot {
    constructor(slot, o) {
      this.size = o.size;
      this.theme = o.theme ?? "atelier";
      this.alive = !!o.alive;
      this.follow = o.follow ?? true;
      this.interactive = !!o.interactive;
      this.bust = o.bust ?? this.size < 40;
      // The canvas reaches past the box so raised arms and hops are never cut.
      this.bleed = this.bust ? 1 : (o.bleed ?? 1.3);
      this.shadow = o.shadow ?? null;
      const el = document.createElement("div");
      el.className = "mz";
      el.style.width = el.style.height = `${this.size}px`;
      el.style.setProperty("--s", `${this.size}px`);
      el.setAttribute("role", "img");
      el.innerHTML = `<div class="mz-move"><canvas></canvas><div class="mz-eyes"><div class="mz-pair"><svg class="mz-eye" viewBox="0 0 20 30" aria-hidden="true"></svg><svg class="mz-eye" viewBox="0 0 20 30" aria-hidden="true"></svg></div><i class="mz-tape"></i><i class="mz-tape"></i></div><div class="mz-fx"></div></div>`;
      slot.replaceChildren(el);
      this.el = el;
      this.move = el.querySelector(".mz-move");
      this.fx = el.querySelector(".mz-fx");
      this.eyesEl = el.querySelector(".mz-eyes");
      this.pairEl = el.querySelector(".mz-pair");
      this.eyeSvgs = [...el.querySelectorAll(".mz-eye")];
      this.tapes = [...el.querySelectorAll(".mz-tape")];
      this.sound = !!o.sound;
      this.eyeKey = "";
      const c = el.querySelector("canvas");
      this.canvas = c;
      if (this.bleed > 1) {
        // The stylesheet pins the canvas to the box; widen it and pull it back to center.
        const o = `${(-(this.bleed - 1) / 2) * 100}%`;
        const w = `${this.bleed * 100}%`;
        Object.assign(c.style, { inset: "auto", left: o, top: o, width: w, height: w });
      }
      this.ctx = c.getContext("2d");
      // Below 40 px only the upper body shows; it fades out at the bottom.
      if (this.bust) c.style.maskImage = c.style.webkitMaskImage = "linear-gradient(180deg, #000 66%, transparent 97%)";
      this.ppu = (this.size * dpr) / 2.21;
      this.setScale(1);
      this.s = {
        y: new Spring(0, 0.5, 0.55),
        tilt: new Spring(0, 0.55, 0.6),
        scale: new Spring(1, 0.45, 0.6),
        squash: new Spring(0, 0.34, 0.32),
        gx: new Spring(0, 0.35, 0.95),
        gy: new Spring(0, 0.35, 0.95),
        lid: new Spring(1, 0.15, 1),
        px: new Spring(0, 0.35, 0.65),
        py: new Spring(0, 0.35, 0.65),
        ...Object.fromEntries(
          ["l", "r"].flatMap((sd) => ["hx", "hy", "hz"].map((c, i) => [sd + c, new Spring(REST[i], 0.42, 0.62)])),
        ),
        mouth: new Spring(0, 0.25, 1),
        blush: new Spring(0, 0.5, 1),
        dim: new Spring(0, 0.6, 1),
        glow: new Spring(1, 0.5, 1),
      };
      this.timers = new Set();
      this.dirty = true;
      this.expr = null;
      this.look = X.idle;
      this.eyes = ["pill", "pill"];
      this.rim = null;
      this.show(o.expr ?? "idle");
      if (o.blink !== false) this.blinkLoop();
      this.aliveTick();
      all.push(this);
    }
    /** Renders sharper when the view is zoomed. */
    setScale(k) {
      const ss = this.size <= 44 ? 3 : this.size <= 130 ? 2.4 : 1.2;
      const px = this.size * this.bleed * dpr * k;
      this.canvas.width = this.canvas.height = Math.round(px);
      this.res = Math.min(700, Math.round(px * (k > 1 ? Math.max(1.5, (ss / k) * 1.2) : ss)));
      this.dirty = true;
    }
    later(ms, fn) {
      const t = setTimeout(() => {
        this.timers.delete(t);
        fn();
      }, ms);
      this.timers.add(t);
      return t;
    }
    setTheme(t) {
      this.theme = t;
      this.dirty = true;
    }
    show(expr) {
      const prev = this.look;
      this.expr = expr;
      const x = (this.look = X[expr]);
      const s = this.s;
      // Eyes change behind a blink so it reads as one face changing.
      const next = Array.isArray(x.eyes) ? x.eyes : [x.eyes, x.eyes];
      if (next.join() !== this.eyes.join()) {
        if (this.expr && !reduced && prev !== x) {
          s.lid.to(0.05);
          this.later(85, () => {
            this.eyes = next;
            s.lid.to(1);
            this.dirty = true;
          });
        } else this.eyes = next;
      }
      const p = x.pose ?? {};
      s.y.to(p.y ?? 0);
      s.tilt.to(p.tilt ?? 0);
      s.scale.to(p.scale ?? 1);
      this.gazeLocked = !!x.gaze;
      if (x.gaze) {
        s.gx.to(x.gaze[0]);
        s.gy.to(x.gaze[1]);
      } else if (prev.gaze) {
        s.gx.to(0);
        s.gy.to(0);
      }
      const arms = ARMS[x.arms ?? "rest"];
      const K = ["hx", "hy", "hz"];
      K.forEach((c, i) => {
        s["l" + c].to(arms[0][i]);
        s["r" + c].to(arms[1][i]);
      });
      if (x.arms === "wave" && !reduced) {
        const id = expr;
        [
          [300, 0.7],
          [500, 1.02],
          [700, 0.7],
          [900, 1.02],
          [1150, 0.92],
        ].forEach(([ms, hx]) =>
          this.later(ms, () => {
            if (this.expr === id) s.rhx.to(hx);
          }),
        );
      }
      this.mouthShape = x.mouth ?? "none";
      clearTimeout(this.mouthT);
      s.mouth.to(x.mouth ? 1 : 0);
      if (x.mouth && x.life !== "talk") this.mouthT = setTimeout(() => s.mouth.to(0), 5000);
      s.blush.to(x.blush ? 1 : 0);
      s.dim.to(x.dim ?? 0);
      s.glow.to(x.glow ?? 1);
      this.rim = x.rim ?? null;
      this.life = reduced ? "still" : (x.life ?? "breathe");
      this.since = performance.now();
      this.evs = new Set();
      this.yawnT0 = 0;
      this.droop = 1;
      for (const tp of this.tapes) tp.style.opacity = "0";
      if (this.life === "fightSleep") this.startYawn();
      if (this.sound) Sound.play(XS[expr]);
      if (reduced) Object.values(s).forEach((sp) => sp.snap());
      else this.accent(x.accent);
      this.effect(x.effect);
      this.dirty = true;
    }
    accent(kind) {
      const s = this.s;
      switch (kind) {
        case "hop":
          s.squash.kick(-1.8);
          this.later(90, () => {
            s.y.kick(-1);
            s.squash.kick(3.4);
          });
          break;
        case "hop2":
          s.squash.kick(-2);
          this.later(90, () => {
            s.y.kick(-1.1);
            s.squash.kick(3.6);
          });
          this.later(420, () => {
            s.y.kick(-0.8);
            s.squash.kick(2.6);
          });
          break;
        case "jump":
          s.y.kick(-1.3);
          s.squash.kick(3);
          s.scale.kick(0.6);
          break;
        case "nod":
          s.tilt.kick(-55);
          s.y.kick(0.25);
          break;
        case "lift":
          s.y.kick(-0.45);
          s.squash.kick(1.6);
          s.tilt.kick(30);
          break;
        case "sink":
          s.y.kick(0.35);
          s.squash.kick(-2.6);
          break;
        case "shiver":
          for (let i = 0; i < 4; i++) this.later(i * 70, () => s.tilt.kick(i % 2 ? 70 : -70));
          break;
        case "hello":
          s.y.kick(-0.5);
          s.squash.kick(1.4);
          break;
        case "wink": {
          let keep;
          this.later(180, () => {
            keep = this.eyes;
            this.eyes = [keep[0], "happy"];
            this.dirty = true;
          });
          this.later(820, () => {
            if (keep && this.eyes[1] === "happy") {
              this.eyes = keep;
              this.dirty = true;
            }
          });
          s.tilt.kick(-30);
          break;
        }
      }
    }
    blink() {
      this.s.lid.to(0.05);
      this.later(130, () => this.s.lid.to(1));
    }
    blinkLoop() {
      if (reduced) return;
      this.later(rnd(2600, 6000), () => {
        const open = !/happy|closed|squint/.test(this.eyes.join());
        if (open && this.s.lid.target === 1) {
          this.blink();
          if (Math.random() < 0.3) this.later(380, () => this.blink());
        }
        this.blinkLoop();
      });
    }
    aliveTick() {
      this.later(rnd(3000, 6500), () => {
        if (this.alive && this.expr === "idle" && performance.now() - lastPointer > 1500) {
          const r = Math.random();
          if (r < 0.55) {
            this.s.gx.to(rnd(-0.9, 0.9));
            this.s.gy.to(rnd(-0.5, 0.4));
            this.later(rnd(900, 1600), () => {
              if (this.expr === "idle") {
                this.s.gx.to(0);
                this.s.gy.to(0);
              }
            });
          } else if (r < 0.75) {
            this.blink();
            this.later(260, () => this.blink());
          } else if (r < 0.82) this.visit("happy", 2200);
          else if (r < 0.9) this.visit("hm", 1600);
          else this.visit("sleepy", 2600);
        }
        this.aliveTick();
      });
    }
    visit(e, ms) {
      const snd = this.sound;
      this.sound = false;
      this.show(e);
      this.sound = snd;
      this.later(ms, () => {
        if (this.expr === e) {
          this.sound = false;
          this.show("idle");
          this.sound = snd;
        }
      });
    }
    effect(kind) {
      clearInterval(this.fxTimer);
      for (const c of [...this.fx.children])
        c.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 200 })
          .finished.then(() => c.remove())
          .catch(() => c.remove());
      if (!kind || reduced || this.size < 40) return;
      if (kind === "dots") {
        const b = document.createElement("div");
        b.className = "mz-bubble";
        for (let i = 0; i < 3; i++) {
          const d = document.createElement("span");
          b.append(d);
          d.animate([{ opacity: 0.35 }, { opacity: 1 }, { opacity: 0.35 }], {
            duration: this.expr === "offline" ? 2000 : 1100,
            iterations: Infinity,
            delay: i * 180,
          });
        }
        this.fx.append(b);
        return;
      }
      if (kind === "nowifi") {
        const b = document.createElement("div");
        b.className = "mz-bubble mz-nowifi";
        b.innerHTML =
          '<svg viewBox="0 0 24 24" width="100%" height="100%" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M2 8.5a15 15 0 0 1 20 0" opacity=".35"/><path d="M5.5 12.2a10 10 0 0 1 13 0" opacity=".55"/><path d="M9 15.8a5 5 0 0 1 6 0"/><circle cx="12" cy="19.3" r="1.3" fill="currentColor" stroke="none"/><path d="M4 3l16 17" stroke="#ff8a7a"/></svg>';
        b.animate(
          [
            { opacity: 0.45, transform: "translateY(0)" },
            { opacity: 1, transform: "translateY(-3px)" },
            { opacity: 0.45, transform: "translateY(0)" },
          ],
          { duration: 2400, iterations: Infinity },
        );
        this.fx.append(b);
        return;
      }
      const spawn = {
        zzz: () => this.glyph("z", 80, 14, rnd(0.09, 0.13), 0.16, -0.3, 2600),
        zzzSoft: () => this.glyph("z", 80, 18, 0.08, 0.12, -0.22, 2400),
        heart: () => this.glyph("♥", 80, 20, 0.11, 0.04, -0.32, 1700, "#ff7aa2"),
        glints: () => this.glint(),
        hm: () => this.glyph("?", 82, 10, 0.16, 0.05, -0.12, 1400),
      }[kind];
      spawn();
      this.fxTimer = setInterval(spawn, { zzz: 1100, zzzSoft: 1800, heart: 900, glints: 360, hm: 5000 }[kind]);
      if (!kind.startsWith("zzz")) {
        const t = this.fxTimer;
        this.later(1700, () => clearInterval(t));
      }
    }
    glyph(ch, x, y, size, dx, dy, dur, color) {
      const g = document.createElement("span");
      g.className = "mz-glyph";
      g.textContent = ch;
      Object.assign(g.style, { left: `${x}%`, top: `${y}%`, fontSize: `${size}em`, color: color ?? "" });
      this.fx.append(g);
      g.animate(
        [
          { transform: "translate(-50%,-50%) scale(.85)", opacity: 0 },
          { opacity: 1, offset: 0.2 },
          { transform: `translate(calc(-50% + ${dx}em), calc(-50% + ${dy}em)) scale(1)`, opacity: 0 },
        ],
        { duration: dur, easing: "cubic-bezier(0.23,1,0.32,1)" },
      )
        .finished.then(() => g.remove())
        .catch(() => g.remove());
    }
    glint() {
      const g = document.createElement("span");
      g.className = "mz-twinkle";
      const a = rnd(-2.6, -0.5),
        r = rnd(0.46, 0.56);
      Object.assign(g.style, {
        left: `${50 + Math.cos(a) * r * 100}%`,
        top: `${46 + Math.sin(a) * r * 100}%`,
        fontSize: `${rnd(0.06, 0.1)}em`,
      });
      this.fx.append(g);
      g.animate(
        [
          { transform: "translate(-50%,-50%) scale(.6) rotate(0deg)", opacity: 0 },
          { transform: "translate(-50%,-50%) scale(1) rotate(45deg)", opacity: 1, offset: 0.35 },
          { transform: "translate(-50%,-50%) scale(.7) rotate(90deg)", opacity: 0 },
        ],
        { duration: 700 },
      )
        .finished.then(() => g.remove())
        .catch(() => g.remove());
    }
    uniforms(now) {
      const t = THEMES[this.theme],
        s = this.s;
      const [el, er] = this.eyes;
      const v = (sd) => ["hx", "hy", "hz"].map((c) => s[sd + c].value);
      const lv = v("l"),
        rv = v("r");
      const [la, lm, lb] = armSeg(-1, lv);
      const [ra, rm, rb] = armSeg(1, rv);
      let mouth = M[this.mouthShape];
      let mh = mouth[2],
        ma = Math.max(0, Math.min(1, s.mouth.value)),
        lid = Math.max(0.05, s.lid.value);
      // A real yawn: the mouth opens wide, the eyes squeeze, then it settles.
      const open = this.yawn(now);
      if (open > 0.001) {
        mouth = [7, 0.04 + 0.05 * open, 0];
        mh = 0.026 + 0.1 * open;
        ma = Math.max(ma, Math.min(1, open * 3));
        lid *= 1 - 0.8 * open;
      }
      if (this.life === "talk") mh *= 0.45 + 0.55 * Math.abs(Math.sin(now / 140) * Math.sin(now / 310 + 1));
      const small = this.size < 60;
      return {
        uYaw: -s.gx.value * 0.34,
        uPitch: -s.gy.value * 0.14,
        uGaze: [s.gx.value, -s.gy.value],
        uArmLA: la,
        uArmLM: lm,
        uArmLB: lb,
        uArmRA: ra,
        uArmRM: rm,
        uArmRB: rb,
        uFur: lin(t.fur),
        uTip: lin(t.tip),
        uRim: this.rim ? mixHex(t.rim, this.rim, 0.75) : lin(t.rim),
        uRimI: t.rimI * (small ? 1.35 : 1) * (this.rim ? 1.25 : 1),
        uKey: [3.8, 3.7, 3.5],
        uDim: s.dim.value,
        uFurAmp: 0.018,
        uDetail: 44,
        uBump: t.bump * 0.52,
        uMicro: Math.min(0.88, t.micro + 0.24),
        uTipK: 0.28,
        uEyeCore: t.core,
        uEyeEdge: t.edge,
        uEmissive: t.emissive,
        uGlow: s.glow.value * (small ? 1.15 : 1),
        uGlint: t.emissive ? 0 : 1,
        uEyeL: [9, 0, 0, 0],
        uEyeR: [9, 0, 0, 0],
        uEyeLB: [LID[el] ?? 1, 0, 0, 0],
        uEyeRB: [LID[er] ?? 1, 0, 0, 0],
        uLid: Math.max(0.05, lid),
        uMouth: [mouth[0], mouth[1], mh, ma],
        uBlush: Math.max(0, s.blush.value),
        uZoom: (this.bust ? 0.78 : 1.06) * this.bleed,
        uCamY: this.bust ? 0.24 : 0.02,
        uWig: 1,
        uWigCol: lin("#1c1714"),
      };
    }
    startYawn() {
      this.yawnT0 = performance.now();
      this.dirty = true;
    }
    /** A real yawn, about 2.6 s: opens on a breath in, holds with a little tremble, closes with an "mm". */
    yawn(now) {
      if (this.yawnT0) {
        const t = (now - this.yawnT0) / 1000;
        if (t > 2.6) return 0;
        const eo = (x) => 1 - (1 - Math.min(1, Math.max(0, x))) ** 3,
          io = (x) => {
            x = Math.min(1, Math.max(0, x));
            return x < 0.5 ? 4 * x * x * x : 1 - (-2 * x + 2) ** 3 / 2;
          };
        if (t < 0.55) return 0.3 * eo(t / 0.55);
        if (t < 1.0) return 0.3 + 0.7 * eo((t - 0.55) / 0.45);
        if (t < 1.75) return 1 + 0.035 * Math.sin((t - 1) * 2 * Math.PI * 5.5);
        if (t < 2.25) return 1 - io((t - 1.75) / 0.5);
        return 0.13 * Math.sin((Math.PI * (t - 2.25)) / 0.35);
      }
      if (this.life !== "yawn") return 0;
      const t = (now + this.size * 97) % 6500;
      if (t > 2200) return 0;
      const ease = (x) => x * x * (3 - 2 * x);
      if (t < 800) return ease(t / 800);
      if (t < 1500) return 1;
      return 1 - ease((t - 1500) / 700);
    }
    scene(now) {
      const e = (x) => {
        x = Math.min(1, Math.max(0, x));
        return x * x * (3 - 2 * x);
      };
      const bump = (x, a, d) => (x > a && x < a + d ? Math.sin((Math.PI * (x - a)) / d) : 0);
      const t = (now - this.since) / 1000;
      let ly = 0,
        lr = 0,
        droop = 1,
        br = 1;
      if (this.life === "nodOff") {
        const p = (t % 4.6) / 4.6;
        droop = p < 0.72 ? 1 - 0.7 * e(p / 0.72) : p < 0.8 ? 0.3 + 0.7 * e((p - 0.72) / 0.08) : 1;
        ly = (1 - droop) * 0.03 - (p >= 0.72 && p < 0.86 ? 0.015 * Math.sin((Math.PI * (p - 0.72)) / 0.14) : 0);
        lr = (1 - droop) * 4;
        if (this.sound && p >= 0.72 && this.mark(Math.floor(t / 4.6) + "catch")) Sound.play("startle");
      } else if (this.life === "fightSleep") {
        // It has to stay up: lids sink and flicker, shut as the head nods, then a "brrr" shake and the eyes pop
        // open. Every third time tape holds the lids up instead, and every other shake ends in another yawn.
        const ct = t - 2.8,
          k = Math.floor(Math.max(0, ct) / 8),
          c = Math.max(0, ct) - k * 8,
          tape = k % 3 === 2;
        const ev = (name, when) => c >= when && ct >= 0 && this.mark(k + name);
        if (ct < 0) droop = 1;
        else if (!tape) {
          if (c < 0.6) droop = 1;
          else if (c < 3) droop = 1 - 0.94 * e((c - 0.6) / 2.4) + 0.28 * bump(c, 1.5, 0.3) + 0.16 * bump(c, 2.3, 0.25);
          else if (c < 5.4) {
            droop = 0.06;
            ly += 0.028 * e((c - 3) / 0.8);
            lr += 7 * e((c - 3) / 1.2) + 2 * Math.sin((c - 3) * 2.2);
          } else if (c < 6.2) {
            const u = (c - 5.4) / 0.8,
              dec = 1 - u;
            lr += 16 * Math.sin(u * 2 * Math.PI * 4.5) * dec + 7 * (1 - e(u * 3));
            ly += 0.028 * (1 - e(u * 4));
            droop = u < 0.08 ? 0.06 : 1.18 - 0.18 * e((u - 0.1) / 0.9);
          } else droop = 1 - 0.12 * e((c - 6.2) / 1.8);
          if (ev("z", 3.3)) {
            this.glyph("z", 74, 14, 0.09, 0.12, -0.24, 2200);
            if (this.sound) Sound.play("doze");
          }
          if (ev("shake", 5.4)) {
            if (this.sound) Sound.play("shake");
            this.s.y.kick(-0.5);
            this.s.squash.kick(1.2);
          }
          if (k % 2 === 1 && ev("yawn", 6.4)) {
            this.startYawn();
            if (this.sound) Sound.play("yawn");
          }
        } else {
          if (c < 0.6) droop = 1;
          else if (c < 2.5) droop = 1 - 0.72 * e((c - 0.6) / 1.9);
          else if (c < 6) {
            const u = c - 2.5;
            droop =
              1.16 + 0.04 * Math.sin(u * 2 * Math.PI * 3.2) - 0.1 * Math.max(0, Math.sin(u * 2 * Math.PI * 0.9)) ** 6;
            const on = e(u / 0.12),
              peel = e((c - 5.6) / 0.4);
            this.tapes.forEach((x, i) => {
              x.style.opacity = String(on * (1 - peel));
              x.style.transform = `translateY(${-6 * peel}%) rotate(${(i ? 1 : -1) * (8 + 50 * peel)}deg) scale(${1.25 - 0.25 * on})`;
            });
            lr += 0.8 * Math.sin(u * 2 * Math.PI * 6);
            br = 1.01;
          } else droop = 1.16 - 0.5 * e((c - 6) / 1.2) + 0.34 * e((c - 7.4) / 0.5);
          if (c < 2.5 || c >= 6) this.tapes.forEach((x) => (x.style.opacity = "0"));
          if (ev("tape", 2.5)) {
            if (this.sound) Sound.play("tapeOn");
            this.s.y.kick(-0.35);
          }
          if (ev("peel", 5.6) && this.sound) Sound.play("tapeOff");
        }
      } else return null;
      this.droop = droop;
      return { ly, lr, breathe: br };
    }
    mark(key) {
      if (this.evs.has(key)) return false;
      this.evs.add(key);
      return true;
    }
    /** Puts the app's eyes on the face, following the 3D turn of the body. */
    placeEyes(now) {
      const s = this.s,
        size = this.size;
      const t = EYE_INK[this.theme];
      const st = this.el.style;
      st.setProperty("--eye", t[0]);
      st.setProperty("--glow", t[1]);
      st.setProperty("--eglow", String(t[2] * Math.max(0.2, s.glow.value)));
      const open = this.yawn(now);
      const shapes = open > 0.35 ? ["yawnL", "yawnR"] : this.eyes;
      const key = shapes.join();
      if (key !== this.eyeKey) {
        this.eyeKey = key;
        this.eyeSvgs[0].innerHTML = APP_EYES[shapes[0]];
        this.eyeSvgs[1].innerHTML = APP_EYES[shapes[1]];
      }
      const zoom = this.bust ? 0.78 : 1.06,
        camY = this.bust ? 0.24 : 0.02;
      const yaw = -s.gx.value * 0.34,
        pitch = -s.gy.value * 0.14;
      const cy = Math.cos(yaw),
        sy = Math.sin(yaw),
        cp = Math.cos(pitch),
        sp = Math.sin(pitch);
      // world = rot^T (obj - pivot) + pivot, rot = rx * ry (as in the shader)
      const R = [
        [cy, 0, sy],
        [sp * sy, cp, -sp * cy],
        [-cp * sy, sp, cp * cy],
      ];
      const toWorld = (o) => {
        const q = [o[0], o[1] + 0.1, o[2]];
        return [
          R[0][0] * q[0] + R[1][0] * q[1] + R[2][0] * q[2],
          R[0][1] * q[0] + R[1][1] * q[1] + R[2][1] * q[2] - 0.1,
          R[0][2] * q[0] + R[1][2] * q[1] + R[2][2] * q[2],
        ];
      };
      const gx = s.gx.value * 0.05,
        gy = -s.gy.value * 0.035;
      const c = toWorld([gx, 0.19 + gy, 0.57]);
      const camZ = 5.4,
        camYw = 0.06 + camY;
      const k = 1 / (camZ - c[2]) / (2 * 0.205 * zoom);
      const ux = c[0] * k,
        uy = (c[1] - camYw) * k;
      const px = (0.5 + ux) * size,
        py = (0.5 - uy) * size;
      const scale = (camZ / (camZ - c[2])) * (2.21 / (2.21 * zoom));
      const unit = size / (2 * 0.205 * zoom * camZ); // px per world unit at the face
      st.setProperty("--ew", `${0.23 * unit * scale}px`);
      st.setProperty("--eh", `${0.345 * unit * scale}px`);
      st.setProperty("--gap", `${0.09 * unit * scale * Math.cos(yaw)}px`);
      const lid =
        Math.max(0.05, s.lid.value) *
        (open > 0.001 && open <= 0.35 ? 1 - 0.8 * open : 1) *
        (open > 0.35 ? 1 : Math.max(0.04, this.droop ?? 1));
      this.eyesEl.style.transform = `translate(${px}px, ${py}px)`;
      // Heavy lids come down from the top, so the pair shrinks toward its lower edge.
      this.pairEl.style.transformOrigin = this.life === "fightSleep" || this.life === "nodOff" ? "50% 78%" : "50% 50%";
      this.pairEl.style.transform = `scaleY(${lid})`;
      const half = `calc((var(--ew) + var(--gap)) / 2)`;
      this.tapes[0].style.left = `calc(-1 * ${half})`;
      this.tapes[1].style.left = half;
    }
    render(now) {
      if (R.draw(this.uniforms(now), this.res, this.res, this.ctx)) this.dirty = false;
    }
    place(now) {
      const s = this.s,
        size = this.size;
      const sq = s.squash.value * 0.08,
        sc = s.scale.value;
      let breathe = 1,
        sway = 0;
      if (this.life === "breathe" || this.life === "talk") breathe = 1 + 0.01 * Math.sin(now / 760);
      if (this.life === "pulse") breathe = 1 + 0.02 * Math.sin(now / 210);
      if (this.life === "sway") sway = 2.2 * Math.sin(now / 800);
      if (this.life === "yawn") {
        sway = 1.8 * Math.sin(now / 900);
        breathe = 1 / (1 + 0.035 * this.yawn(now));
      }
      let ly = 0;
      const sc2 = this.scene(now);
      if (sc2) {
        ly = sc2.ly;
        sway += sc2.lr;
        breathe *= sc2.breathe;
      }
      const yo = this.yawn(now);
      if (yo > 0.001 && this.yawnT0) {
        ly -= 0.03 * Math.min(1, yo);
        breathe *= 1 / (1 + 0.05 * Math.min(1, yo));
        sway -= 3 * Math.min(1, yo);
      }
      this.move.style.transform = `translate(${s.px.value}px, ${s.py.value + (s.y.value + ly) * size}px) rotate(${s.tilt.value + sway}deg) scale(${sc * (1 + sq) * breathe}, ${(sc * (1 - sq)) / breathe})`;
      this.placeEyes(now);
      if (this.shadow) {
        const lift = Math.max(0, -s.y.value) * 6;
        this.shadow.style.scale = `${1 / (1 + lift)}`;
        this.shadow.style.opacity = String(1 / (1 + lift * 1.6));
      }
    }
  }

  // One loop for every creature; it sleeps while none is on screen.
  const shaderKeys = ["gx", "gy", "lid", "lhx", "lhy", "lhz", "rhx", "rhy", "rhz", "mouth", "blush", "dim", "glow"];
  let last = performance.now();
  let cursor = 0;
  let raf = 0;
  function frame(now) {
    raf = 0;
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    const t0 = performance.now();
    // Start where the last frame stopped, so a busy creature never starves the rest.
    const order = all.slice(cursor).concat(all.slice(0, cursor));
    cursor = (cursor + 1) % Math.max(1, all.length);
    let busy = false;
    for (const m of order) {
      if (!m.active) continue;
      busy = true;
      const springs = m.s;
      let moving = false;
      for (const k in springs) {
        const sp = springs[k];
        if (sp.resting) continue;
        const steps = Math.max(1, Math.ceil(dt / (1 / 120)));
        for (let i = 0; i < steps; i++) sp.step(dt / steps);
        if (shaderKeys.includes(k)) moving = true;
      }
      if (
        moving ||
        m.life === "talk" ||
        ((m.life === "yawn" || m.yawnT0) && (m.yawn(now) > 0 || now - m.yawnT0 < 2700))
      )
        m.dirty = true;
      m.place(now);
      if (m.dirty && performance.now() - t0 < 12) m.render(now);
    }
    if (busy && !document.hidden) raf = requestAnimationFrame(frame);
  }
  const wake = () => {
    if (!raf && all.length) {
      last = performance.now();
      raf = requestAnimationFrame(frame);
    }
  };
  document.addEventListener("visibilitychange", wake);

  Mascot.prototype.gaze = function (x, y) {
    lastPointer = performance.now();
    if (this.gazeLocked) return;
    this.s.gx.to(x);
    this.s.gy.to(y);
    wake();
  };
  Mascot.prototype.pull = function (x, y) {
    if (reduced) return;
    this.s.px.to(x);
    this.s.py.to(y);
    wake();
  };
  Mascot.prototype.setActive = function (on) {
    this.active = on;
    if (on) {
      this.dirty = true;
      wake();
    }
  };
  Mascot.prototype.setAlive = function (on) {
    this.alive = on;
  };
  Mascot.prototype.destroy = function () {
    for (const t of this.timers) clearTimeout(t);
    clearTimeout(this.mouthT);
    clearInterval(this.fxTimer);
    clearTimeout(this.release);
    this.timers.clear();
    const i = all.indexOf(this);
    if (i >= 0) all.splice(i, 1);
    this.el.remove();
  };
  // Any change of mood or colour wakes the loop.
  for (const k of ["show", "setTheme", "later"]) {
    const f = Mascot.prototype[k];
    Mascot.prototype[k] = function (...a) {
      const r = f.apply(this, a);
      wake();
      return r;
    };
  }
  return { Mascot, THEMES, EXPRESSIONS: X, wake };
}

/** True when this machine can draw the creature (WebGL 2). */
export function creatureSupported() {
  if (!api && !failed && typeof window !== "undefined") api = boot();
  return !!api;
}

export function createCreature(slot, opts) {
  if (!creatureSupported()) return null;
  const m = new api.Mascot(slot, { follow: false, ...opts });
  m.active = opts.active ?? true;
  api.wake();
  return m;
}
