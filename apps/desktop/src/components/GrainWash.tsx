"use client";

// A slow grain gradient that retints on a spring when something happens and
// sends a soft ring out from `origin`. It draws a still frame when idle and
// hidden, and only runs the animation loop while something is moving.

import { animate } from "motion/react";
import type { Ref } from "react";
import { useEffect, useImperativeHandle, useRef } from "react";
import { useReduced } from "@/lib/use-reduced";
import { channels, PALETTES, type WashEvent } from "@/lib/wash";

export interface GrainWashHandle {
  /** Retints to the event and sends a ring; transient events settle back to `base`. */
  fire: (event: WashEvent) => void;
}

const VERT = "attribute vec2 p;void main(){gl_Position=vec4(p,0.,1.);}";
const FRAG = `precision mediump float;
uniform vec2 r;uniform float t;uniform vec3 a;uniform vec3 b;uniform vec3 c;uniform float e;uniform vec4 pu;uniform float g;uniform float gt;
float h(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
float n(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.-2.*f);return mix(mix(h(i),h(i+vec2(1.,0.)),f.x),mix(h(i+vec2(0.,1.)),h(i+vec2(1.,1.)),f.x),f.y);}
float fbm(vec2 p){float s=0.,m=.5;for(int i=0;i<5;i++){s+=m*n(p);p=p*2.03+vec2(1.7,9.2);m*=.5;}return s;}
void main(){
 vec2 uv=gl_FragCoord.xy/r; float asp=r.x/r.y; vec2 q=vec2(uv.x*asp,uv.y);
 float tt=t*.05;
 vec2 w=vec2(fbm(q*1.1+vec2(tt,tt*.6)),fbm(q*1.1-vec2(tt*.8,-tt)+4.));
 float f1=fbm(q*1.05+w*1.7+vec2(tt*.6,-tt*.4));
 float f2=fbm(q*1.5-w*1.3+vec2(-tt*.5,tt*.7)+7.);
 vec3 col=vec3(.03,.033,.031);
 float top=smoothstep(1.,0.,length((uv-vec2(.5,1.04))*vec2(1.05,1.9)));
 col+=a*smoothstep(.38,.95,f1)*(.12+.42*e);
 col+=b*smoothstep(.48,1.,f2)*(.08+.34*e);
 col+=mix(a,c,.5)*top*(.05+.2*e);
 vec2 pp=vec2(pu.x,1.-pu.y); float d=length((uv-pp)*vec2(asp,1.));
 float age=pu.z; float rad=age*.62;
 float ring=exp(-pow((d-rad)*6.,2.))*pu.w*exp(-age*1.5);
 col+=(a*.55+c*.12)*ring;
 col+=a*exp(-d*3.5)*pu.w*exp(-age*2.6)*.35;
 col*=mix(.6,1.,smoothstep(1.3,.2,length(uv-vec2(.5,.62))));
 col+=(h(gl_FragCoord.xy+gt*91.7)-.5)*g;
 gl_FragColor=vec4(col,1.);
}`;

const UNIFORMS = ["r", "t", "a", "b", "c", "e", "pu", "g", "gt"] as const;
/** Seconds a ring needs to fade out; the loop can stop after this. */
const RING_SECONDS = 3.2;
const COLOR_SPRING = { type: "spring", stiffness: 49, damping: 12.6, mass: 1 } as const;
const COLOR_FADE = { duration: 0.3 } as const;

export function GrainWash({
  base,
  origin = { x: 0.5, y: 0.06 },
  grain = 0.45,
  className,
  handle,
}: {
  /** The tint to hold between events. */
  base: WashEvent;
  /** Where rings start, as fractions of the width and height. */
  origin?: { x: number; y: number };
  grain?: number;
  className?: string;
  handle?: Ref<GrainWashHandle>;
}) {
  const reduced = useReduced();
  const canvas = useRef<HTMLCanvasElement>(null);
  const live = useRef<{ fire: (e: WashEvent) => void; setBase: (e: WashEvent) => void } | null>(null);
  const baseRef = useRef(base);
  const originRef = useRef(origin);
  originRef.current = origin;
  const reducedRef = useRef(reduced);
  reducedRef.current = reduced;

  useImperativeHandle(handle, () => ({ fire: (e) => live.current?.fire(e) }), []);

  useEffect(() => {
    baseRef.current = base;
    live.current?.setBase(base);
  }, [base]);

  useEffect(() => {
    const cv = canvas.current;
    if (!cv) return;
    const gl = cv.getContext("webgl", {
      antialias: false,
      premultipliedAlpha: false,
      powerPreference: "low-power",
    });
    if (!gl) return;

    const compile = (type: number, src: string) => {
      const s = gl.createShader(type);
      if (!s) throw new Error("no shader");
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? "shader");
      return s;
    };
    const program = gl.createProgram();
    try {
      gl.attachShader(program, compile(gl.VERTEX_SHADER, VERT));
      gl.attachShader(program, compile(gl.FRAGMENT_SHADER, FRAG));
    } catch {
      return;
    }
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return;
    // biome-ignore lint/correctness/useHookAtTopLevel: WebGL's useProgram, not a React hook
    gl.useProgram(program);
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(program, "p");
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    const u = Object.fromEntries(UNIFORMS.map((k) => [k, gl.getUniformLocation(program, k)])) as Record<
      (typeof UNIFORMS)[number],
      WebGLUniformLocation | null
    >;

    // Current animated values.
    const col = channels(PALETTES[baseRef.current]);
    let energy = PALETTES[baseRef.current].energy;
    let ring = { t0: -1e6, s: 0 };
    let time = 0;
    let running = false;
    let raf = 0;
    let settle = 0;
    let moving = 0; // animations in flight

    const draw = (now: number) => {
      gl.viewport(0, 0, cv.width, cv.height);
      gl.uniform2f(u.r, cv.width, cv.height);
      gl.uniform1f(u.t, time);
      gl.uniform3f(u.a, col[0], col[1], col[2]);
      gl.uniform3f(u.b, col[3], col[4], col[5]);
      gl.uniform3f(u.c, col[6], col[7], col[8]);
      gl.uniform1f(u.e, energy);
      const age = reducedRef.current ? 99 : (now - ring.t0) / 1000;
      const o = originRef.current;
      gl.uniform4f(u.pu, o.x, o.y, age, ring.s);
      gl.uniform1f(u.g, grain * 0.16);
      gl.uniform1f(u.gt, reducedRef.current ? 0 : Math.floor(now / 42) % 97);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    };

    const resize = () => {
      const w = Math.max(2, Math.round(cv.clientWidth * 0.5));
      const h = Math.max(2, Math.round(cv.clientHeight * 0.5));
      if (cv.width !== w || cv.height !== h) {
        cv.width = w;
        cv.height = h;
      }
      draw(performance.now());
    };

    // The loop only runs while the page is visible, motion is allowed and
    // something is moving: a color change, or a ring still fading.
    const needsLoop = (now: number) =>
      !document.hidden && !reducedRef.current && (moving > 0 || (now - ring.t0) / 1000 < RING_SECONDS);

    const frame = (now: number) => {
      raf = 0;
      if (!running) return;
      time += 0.016 * (0.25 + 0.4 * 1.9);
      draw(now);
      if (needsLoop(now)) raf = requestAnimationFrame(frame);
      else running = false;
    };
    const wake = () => {
      if (running || !needsLoop(performance.now())) {
        if (!running) draw(performance.now());
        return;
      }
      running = true;
      raf = requestAnimationFrame(frame);
    };

    const paint = (name: WashEvent, fast: boolean) => {
      const p = PALETTES[name];
      const target = channels(p);
      const transition = reducedRef.current || fast ? COLOR_FADE : COLOR_SPRING;
      moving++;
      let left = target.length + 1;
      const done = () => {
        if (--left === 0) moving--;
      };
      target.forEach((to, i) => {
        animate(col[i], to, {
          ...transition,
          onUpdate: (v) => {
            col[i] = v;
          },
          onComplete: done,
        });
      });
      animate(energy, p.energy, {
        ...transition,
        onUpdate: (v) => {
          energy = v;
        },
        onComplete: done,
      });
      wake();
    };

    live.current = {
      setBase: (name) => {
        window.clearTimeout(settle);
        paint(name, false);
        if (PALETTES[name].ring > 0) ring = { t0: performance.now(), s: PALETTES[name].ring };
      },
      fire: (name) => {
        window.clearTimeout(settle);
        paint(name, false);
        ring = { t0: performance.now(), s: PALETTES[name].ring };
        wake();
        const back = PALETTES[name].settleMs;
        if (back > 0) settle = window.setTimeout(() => paint(baseRef.current, false), back);
      },
    };

    const observer = new ResizeObserver(resize);
    observer.observe(cv);
    const onVisible = () => wake();
    document.addEventListener("visibilitychange", onVisible);
    resize();
    // The first frame is the base tint, drawn without animating in.
    draw(performance.now());

    return () => {
      window.clearTimeout(settle);
      cancelAnimationFrame(raf);
      observer.disconnect();
      document.removeEventListener("visibilitychange", onVisible);
      live.current = null;
      gl.getExtension("WEBGL_lose_context")?.loseContext();
    };
  }, [grain]);

  return <canvas ref={canvas} aria-hidden className={className} style={{ width: "100%", height: "100%" }} />;
}
