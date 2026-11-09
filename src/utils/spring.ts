const MAX_STEP = 1 / 240

export class Spring {
  value: number
  target: number
  velocity = 0

  constructor(
    value: number,
    readonly dampingRatio: number,
    readonly stiffness: number,
    readonly threshold = 0.001,
  ) {
    this.value = value
    this.target = value
  }

  get settled(): boolean {
    return Math.abs(this.value - this.target) < this.threshold && Math.abs(this.velocity) < this.threshold * 10
  }

  snap(value = this.target) {
    this.value = value
    this.target = value
    this.velocity = 0
  }

  step(seconds: number) {
    if (this.settled) {
      this.value = this.target
      this.velocity = 0
      return
    }
    const damping = 2 * this.dampingRatio * Math.sqrt(this.stiffness)
    let remaining = seconds
    while (remaining > 0) {
      const h = Math.min(remaining, MAX_STEP)
      const acceleration = -this.stiffness * (this.value - this.target) - damping * this.velocity
      this.velocity += acceleration * h
      this.value += this.velocity * h
      remaining -= h
    }
  }
}
