// A spring in Apple's terms: `response` is roughly how long it takes to get
// there (seconds) and `damping` is the damping ratio (1 settles without
// overshoot, lower gives a little give). It keeps its velocity when the
// target changes, so motion can be interrupted at any moment.

export class Spring {
  value: number;
  target: number;
  velocity = 0;
  private k = 0;
  private c = 0;

  constructor(value: number, response = 0.4, damping = 1) {
    this.value = value;
    this.target = value;
    this.tune(response, damping);
  }

  tune(response: number, damping: number): this {
    const w = (2 * Math.PI) / response;
    this.k = w * w;
    this.c = 2 * damping * w;
    return this;
  }

  to(target: number): this {
    this.target = target;
    return this;
  }

  /** Adds velocity: a physical nudge that settles on its own. */
  kick(velocity: number): this {
    this.velocity += velocity;
    return this;
  }

  /** Jumps to the target with no motion (reduced motion). */
  snap(): this {
    this.value = this.target;
    this.velocity = 0;
    return this;
  }

  step(dt: number) {
    const a = -this.k * (this.value - this.target) - this.c * this.velocity;
    this.velocity += a * dt;
    this.value += this.velocity * dt;
  }

  get resting(): boolean {
    return Math.abs(this.value - this.target) < 1e-4 && Math.abs(this.velocity) < 1e-3;
  }
}
