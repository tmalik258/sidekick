// Drives one orb straight on its DOM nodes: springs for the pose, gaze and
// eyelids; one nudge per new expression; eyes that change behind a blink; a
// mouth that pops in and fades; small effects. The frame loop runs only while
// a spring is moving. Slow loops (breathing, pulse, sway) are CSS animations.

import { EXPRESSIONS, type Expression, EYES, type Look, MOUTHS } from "./expressions";
import { Spring } from "./spring";

export interface Material {
  body: [string, string];
  halo: [string, string, string];
  haloOpacity: number;
  eye: string;
  eyeGlow: string;
  /** A layer over the body's gradient: an iridescent or metal sheen. */
  sheen?: string;
}

export interface OrbNodes {
  root: HTMLElement;
  halo: HTMLElement;
  /** Holds the halo; moves with the body's position only. */
  haloPos: HTMLElement;
  float: HTMLElement;
  life: HTMLElement;
  body: HTMLElement;
  face: HTMLElement;
  eyes: HTMLElement;
  eyeL: SVGSVGElement;
  eyeR: SVGSVGElement;
  mouth: SVGSVGElement;
  cheeks: HTMLElement[];
  fx: HTMLElement;
}

/** How long the mouth and effects stay after something happens. */
const REACTION_MS = 3000;
/** Effects need room; at compact sizes they would be noise. */
const FX_MIN_SIZE = 40;
const IDLE_SPIN_MS = 14_000;
const EASE_OUT = "cubic-bezier(0.23, 1, 0.32, 1)";

const rnd = (a: number, b: number) => a + Math.random() * (b - a);
const pair = (look: Look) => (Array.isArray(look.eyes) ? look.eyes : [look.eyes, look.eyes]);

export class OrbEngine {
  private s = {
    y: new Spring(0, 0.45, 0.62),
    tilt: new Spring(0, 0.5, 0.7),
    scale: new Spring(1, 0.4, 0.7),
    squash: new Spring(0, 0.3, 0.5),
    gx: new Spring(0, 0.28, 0.85),
    gy: new Spring(0, 0.28, 0.85),
    lid: new Spring(1, 0.09, 1),
    px: new Spring(0, 0.3, 0.6),
    py: new Spring(0, 0.3, 0.6),
  };
  private expr: Expression | null = null;
  private look: Look = EXPRESSIONS.idle;
  private shape = "";
  private material: Material | null = null;
  private spin: Animation;
  private raf = 0;
  private last = 0;
  private timers = new Set<ReturnType<typeof setTimeout>>();
  private reaction: ReturnType<typeof setTimeout> | undefined;
  private fxTimer: ReturnType<typeof setInterval> | undefined;
  private gazeLocked = false;
  private alive = false;
  private aliveTimer: ReturnType<typeof setTimeout> | undefined;
  private lastCursor = 0;
  private destroyed = false;

  constructor(
    private n: OrbNodes,
    private size: number,
    private reduced: boolean,
  ) {
    this.spin = n.halo.animate([{ transform: "rotate(0turn)" }, { transform: "rotate(1turn)" }], {
      duration: IDLE_SPIN_MS,
      iterations: Number.POSITIVE_INFINITY,
    });
    this.blinkLoop();
  }

  destroy() {
    this.destroyed = true;
    cancelAnimationFrame(this.raf);
    this.spin.cancel();
    for (const t of this.timers) clearTimeout(t);
    clearTimeout(this.reaction);
    clearTimeout(this.aliveTimer);
    clearInterval(this.fxTimer);
  }

  setMaterial(m: Material) {
    this.material = m;
    const st = this.n.root.style;
    st.setProperty("--o1", m.body[0]);
    st.setProperty("--eye", m.eye);
    st.setProperty("--eye-glow", m.eyeGlow);
    st.setProperty("--sheen", m.sheen ?? "none");
    this.paint();
  }

  /** Shows an expression: colors at once, pose on springs, one nudge. */
  show(expr: Expression) {
    if (expr === this.expr) return;
    const prev = this.look;
    this.expr = expr;
    this.look = EXPRESSIONS[expr];
    const look = this.look;
    this.paint();
    this.n.life.dataset.life = this.reduced ? "still" : (look.life ?? "breathe");

    // Eyes change behind a quick blink, so it reads as one face changing.
    const shape = pair(look).join();
    if (shape !== this.shape) {
      if (this.shape && !this.reduced) {
        this.s.lid.to(0.06);
        this.later(85, () => {
          this.setEyes();
          this.s.lid.to(1);
          this.wake();
        });
      } else this.setEyes();
      this.shape = shape;
    }

    const p = look.pose ?? {};
    this.s.y.to(p.y ?? 0);
    this.s.tilt.to(p.tilt ?? 0);
    this.s.scale.to(p.scale ?? 1);
    this.gazeLocked = !!look.gaze;
    if (look.gaze) {
      this.s.gx.to(look.gaze[0]);
      this.s.gy.to(look.gaze[1]);
    } else if (prev.gaze) {
      this.s.gx.to(0);
      this.s.gy.to(0);
    }

    this.n.mouth.innerHTML = look.mouth ? MOUTHS[look.mouth] : "";
    this.react();
    if (this.reduced) for (const sp of Object.values(this.s)) sp.snap();
    else this.accent(look.accent);
    this.effect(look.effect);
    this.wake();
  }

  /** Eyes follow a point (-1..1 each way), unless the expression looks somewhere. */
  gaze(x: number, y: number) {
    this.lastCursor = performance.now();
    if (this.gazeLocked) return;
    this.s.gx.to(x);
    this.s.gy.to(y);
    this.wake();
  }

  /** Magnetic pull toward a nearby cursor, in pixels. */
  pull(x: number, y: number) {
    this.s.px.to(x);
    this.s.py.to(y);
    this.wake();
  }

  /** Alive mode: while idle, it glances around, blinks, smiles now and then. */
  setAlive(on: boolean) {
    this.alive = on && !this.reduced;
    clearTimeout(this.aliveTimer);
    if (this.alive) this.aliveTick();
  }

  // ---------- internals ----------

  private later(ms: number, fn: () => void) {
    const t = setTimeout(() => {
      this.timers.delete(t);
      if (!this.destroyed) fn();
    }, ms);
    this.timers.add(t);
    return t;
  }

  private paint() {
    const m = this.material;
    if (!m) return;
    const look = this.look;
    const st = this.n.root.style;
    const halo = look.halo ?? m.halo;
    st.setProperty("--g1", halo[0]);
    st.setProperty("--g2", halo[1]);
    st.setProperty("--g3", halo[2]);
    st.setProperty("--halo", String(look.haloOpacity ?? m.haloOpacity));
    st.setProperty("--o2", look.tint ? `color-mix(in oklab, ${m.body[1]} 72%, ${look.tint})` : m.body[1]);
    this.n.root.dataset.dim = look.dim ? "true" : "false";
    for (const c of this.n.cheeks) c.style.opacity = look.blush ? "0.5" : "0";
    if (this.reduced || look.spin === 0) this.spin.pause();
    else {
      this.spin.updatePlaybackRate(look.spin ?? 1);
      this.spin.play();
    }
  }

  private setEyes() {
    for (const e of [this.n.eyeL, this.n.eyeR]) for (const a of e.getAnimations()) a.cancel();
    const [l, r] = pair(this.look);
    this.n.eyeL.innerHTML = EYES[l];
    this.n.eyeR.innerHTML = EYES[r];
  }

  /** Mouth pops in, fades after a few seconds, and comes back now and then. */
  private react() {
    clearTimeout(this.reaction);
    const show = (on: boolean) => {
      this.n.mouth.dataset.on = on && this.look.mouth ? "true" : "false";
    };
    show(true);
    const peek = () => {
      this.reaction = setTimeout(
        () => {
          show(true);
          this.reaction = setTimeout(
            () => {
              show(false);
              peek();
            },
            rnd(1300, 2000),
          );
        },
        rnd(5000, 11000),
      );
    };
    this.reaction = setTimeout(() => {
      show(false);
      peek();
    }, REACTION_MS);
  }

  /** One physical nudge. Velocities: size/s for y, deg/s for tilt. */
  private accent(kind: Look["accent"]) {
    const s = this.s;
    switch (kind) {
      case "hop":
        s.y.kick(-0.9);
        s.squash.kick(2.2);
        break;
      case "hop2":
        s.y.kick(-1);
        s.squash.kick(2.4);
        this.later(260, () => {
          s.y.kick(-0.7);
          s.squash.kick(1.6);
          this.wake();
        });
        break;
      case "jump":
        s.y.kick(-1.3);
        s.scale.kick(0.6);
        break;
      case "nod":
        s.tilt.kick(-55);
        s.y.kick(0.25);
        break;
      case "lift":
        s.y.kick(-0.45);
        s.tilt.kick(30);
        break;
      case "sink":
        s.y.kick(0.35);
        s.squash.kick(-1.6);
        break;
      case "shiver":
        for (let i = 0; i < 4; i++)
          this.later(i * 70, () => {
            s.tilt.kick(i % 2 ? 70 : -70);
            this.wake();
          });
        break;
      case "hello":
        s.y.kick(-0.6);
        for (let i = 0; i < 3; i++)
          this.later(60 + i * 150, () => {
            s.tilt.kick(i % 2 ? 90 : -90);
            this.wake();
          });
        break;
      case "wink":
        this.later(140, () => this.wink());
        break;
    }
  }

  /** A real wink: one eye shuts into a curve and opens again; the other squints. */
  private wink() {
    const shut = this.n.eyeR;
    const other = this.n.eyeL;
    const still = this.expr;
    shut
      .animate([{ transform: "scaleY(1)" }, { transform: "scaleY(.1)" }], { duration: 90, fill: "forwards" })
      .finished.then(() => {
        if (this.expr !== still) return;
        shut.innerHTML = EYES.happy;
        shut.animate([{ transform: "scaleY(.5)" }, { transform: "scaleY(1)" }], {
          duration: 160,
          easing: EASE_OUT,
          fill: "forwards",
        });
        other.animate(
          [
            { transform: "scaleY(1)" },
            { transform: "scaleY(.82)", offset: 0.25 },
            { transform: "scaleY(.82)", offset: 0.75 },
            { transform: "scaleY(1)" },
          ],
          { duration: 620, easing: EASE_OUT },
        );
        this.s.tilt.kick(-45);
        this.wake();
        if (this.size >= FX_MIN_SIZE) this.glint();
        this.later(520, () => {
          if (this.expr !== still) return;
          shut
            .animate([{ transform: "scaleY(1)" }, { transform: "scaleY(.1)" }], { duration: 80, fill: "forwards" })
            .finished.then(() => {
              shut.innerHTML = EYES.pill;
              shut.animate([{ transform: "scaleY(.1)" }, { transform: "scaleY(1)" }], {
                duration: 180,
                easing: EASE_OUT,
                fill: "forwards",
              });
            })
            .catch(() => {});
        });
      })
      .catch(() => {});
  }

  private blinkLoop() {
    if (this.reduced) return;
    this.later(rnd(2800, 6200), () => {
      if (/pill|wide|big|small|focus|dot|worried/.test(this.shape) && this.s.lid.target === 1) {
        this.blink();
        if (Math.random() < 0.18) this.later(230, () => this.blink());
      }
      this.blinkLoop();
    });
  }

  private blink() {
    this.s.lid.to(0.06);
    this.wake();
    this.later(70, () => {
      this.s.lid.to(1);
      this.wake();
    });
  }

  private aliveTick() {
    this.aliveTimer = setTimeout(
      () => {
        if (!this.alive || this.destroyed) return;
        const quiet = performance.now() - this.lastCursor > 1500;
        if (this.expr === "idle" && quiet) {
          const r = Math.random();
          const night = new Date().getHours() < 5;
          if (r < 0.55) {
            // A glance somewhere, a hold, back.
            this.s.gx.to(rnd(-0.9, 0.9));
            this.s.gy.to(rnd(-0.5, 0.4));
            this.later(rnd(900, 1600), () => {
              if (this.expr !== "idle") return;
              this.s.gx.to(0);
              this.s.gy.to(0);
              this.wake();
            });
          } else if (r < 0.75) {
            this.blink();
            this.later(260, () => this.blink());
          } else if (r < (night ? 0.8 : 0.9)) {
            this.visit("happy", 2200);
          } else {
            this.visit("sleepy", 2600);
          }
          this.wake();
        }
        this.aliveTick();
      },
      rnd(3000, 6500),
    );
  }

  /** Shows an expression for a moment, then goes back to idle. */
  private visit(expr: Expression, ms: number) {
    this.show(expr);
    this.later(ms, () => {
      if (this.expr === expr) this.show("idle");
    });
  }

  private effect(kind: Look["effect"]) {
    clearInterval(this.fxTimer);
    this.fxTimer = undefined;
    for (const c of [...this.n.fx.children]) {
      c.animate([{ opacity: getComputedStyle(c).opacity }, { opacity: 0 }], { duration: 200 })
        .finished.then(() => c.remove())
        .catch(() => c.remove());
    }
    if (!kind || this.reduced || this.size < FX_MIN_SIZE) return;
    if (kind === "dots" || kind === "offline") {
      const b = document.createElement("div");
      b.className = kind === "offline" ? "orb-bubble orb-bubble-muted" : "orb-bubble";
      for (let i = 0; i < 3; i++) {
        const d = document.createElement("span");
        b.append(d);
        d.animate([{ opacity: 0.35 }, { opacity: 1 }, { opacity: 0.35 }], {
          duration: kind === "offline" ? 2000 : 1100,
          iterations: Number.POSITIVE_INFINITY,
          delay: i * 180,
          easing: "ease-in-out",
        });
      }
      this.n.fx.append(b);
      b.animate(
        [
          { opacity: 0, transform: "scale(.9)" },
          { opacity: 1, transform: "none" },
        ],
        {
          duration: 260,
          easing: EASE_OUT,
        },
      );
      return;
    }
    const spawn = {
      zzz: () => this.glyph("z", 76, 10, rnd(0.09, 0.13), 0.16, -0.3, 2600),
      zzzSoft: () => this.glyph("z", 78, 14, 0.08, 0.12, -0.22, 2400),
      heart: () => this.glyph("♥", 80, 20, 0.11, 0.04, -0.32, 1700, "#ff6f96"),
      glints: () => this.glint(),
    }[kind];
    const every = { zzz: 1100, zzzSoft: 1800, heart: 900, glints: 380 }[kind];
    spawn();
    this.fxTimer = setInterval(spawn, every);
    // Reactions end; states (sleep) keep going.
    if (!kind.startsWith("zzz")) {
      const t = this.fxTimer;
      this.later(kind === "heart" ? 1900 : 1600, () => {
        if (this.fxTimer === t) {
          clearInterval(t);
          this.fxTimer = undefined;
        }
      });
    }
  }

  private glyph(ch: string, x: number, y: number, size: number, dx: number, dy: number, dur: number, color?: string) {
    const g = document.createElement("span");
    g.className = "orb-glyph";
    g.textContent = ch;
    Object.assign(g.style, { left: `${x}%`, top: `${y}%`, fontSize: `${size}em`, color: color ?? "" });
    this.n.fx.append(g);
    g.animate(
      [
        { transform: "translate(-50%,-50%) scale(.85)", opacity: 0 },
        { opacity: 1, offset: 0.2 },
        { transform: `translate(calc(-50% + ${dx}em), calc(-50% + ${dy}em)) scale(1)`, opacity: 0 },
      ],
      { duration: dur, easing: EASE_OUT },
    )
      .finished.then(() => g.remove())
      .catch(() => g.remove());
  }

  private glint() {
    const g = document.createElement("span");
    g.className = "orb-twinkle";
    const a = rnd(-2.4, -0.7);
    const r = rnd(0.56, 0.64);
    Object.assign(g.style, {
      left: `${50 + Math.cos(a) * r * 100}%`,
      top: `${50 + Math.sin(a) * r * 100}%`,
      fontSize: `${rnd(0.06, 0.1)}em`,
    });
    this.n.fx.append(g);
    g.animate(
      [
        { transform: "translate(-50%,-50%) scale(.6) rotate(0deg)", opacity: 0 },
        { transform: "translate(-50%,-50%) scale(1) rotate(45deg)", opacity: 1, offset: 0.35 },
        { transform: "translate(-50%,-50%) scale(.7) rotate(90deg)", opacity: 0 },
      ],
      { duration: 700, easing: EASE_OUT },
    )
      .finished.then(() => g.remove())
      .catch(() => g.remove());
  }

  /** Starts the frame loop if it is not running. */
  private wake() {
    if (this.raf || this.destroyed) return;
    this.last = performance.now();
    this.raf = requestAnimationFrame(this.frame);
  }

  private frame = (now: number) => {
    const dt = Math.min(0.05, (now - this.last) / 1000);
    this.last = now;
    const steps = Math.max(1, Math.ceil(dt / (1 / 120)));
    const springs = Object.values(this.s);
    for (let i = 0; i < steps; i++) for (const sp of springs) sp.step(dt / steps);
    this.render();
    this.raf = springs.every((sp) => sp.resting) ? 0 : requestAnimationFrame(this.frame);
  };

  private render() {
    const s = this.s;
    const size = this.size;
    const sq = s.squash.value * 0.04;
    const sc = s.scale.value;
    const tx = s.px.value;
    const ty = s.py.value + s.y.value * size;
    this.n.float.style.transform = `translate(${tx}px, ${ty}px) rotate(${s.tilt.value}deg) scale(${sc * (1 + sq)}, ${sc * (1 - sq)})`;
    this.n.haloPos.style.transform = `translate(${tx}px, ${ty}px)`;
    const gx = s.gx.value;
    const gy = s.gy.value;
    this.n.face.style.transform = `translate(${gx * size * 0.13}px, ${gy * size * 0.1}px)`;
    this.n.body.style.transform = `rotateY(${gx * 14}deg) rotateX(${-gy * 14}deg)`;
    this.n.eyes.style.transform = `scaleY(${Math.max(0.04, s.lid.value)})`;
  }
}
