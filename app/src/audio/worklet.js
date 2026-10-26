class CitrineOutput extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const o = options.processorOptions;
    this.target = o.target;
    this.excess = o.excess;
    this.fade = o.fade;
    this.capacity = o.capacity;
    this.ring = new Float32Array(o.capacity * 2);
    this.read = 0;
    this.queued = 0;
    this.received = 0;
    this.playing = false;
    this.started = false;
    this.paused = false;
    this.fadeIn = 0;
    this.blend = new Float32Array(o.fade * 2);
    this.blendPos = o.fade;
    this.left = 0;
    this.right = 0;
    this.underruns = 0;
    this.underrunFrames = 0;
    this.overruns = 0;
    this.overrunFrames = 0;
    this.min = Infinity;
    this.max = -Infinity;
    this.ticks = 0;
    this.port.onmessage = (event) => this.receive(event.data);
  }

  receive(data) {
    if (data instanceof Float32Array) {
      this.push(data);
    } else if (data.reset) {
      this.underruns = 0;
      this.underrunFrames = 0;
      this.overruns = 0;
      this.overrunFrames = 0;
      this.min = Infinity;
      this.max = -Infinity;
    } else if (typeof data.paused === "boolean") {
      this.paused = data.paused;
    }
  }

  push(samples) {
    const frames = samples.length >> 1;
    this.received += frames;
    const take = Math.min(frames, this.capacity - this.queued);
    if (take < frames) {
      this.overruns++;
      this.overrunFrames += frames - take;
    }
    let write = (this.read + this.queued) % this.capacity;
    for (let i = 0; i < take; i++) {
      this.ring[write * 2] = samples[i * 2];
      this.ring[write * 2 + 1] = samples[i * 2 + 1];
      write = write + 1 === this.capacity ? 0 : write + 1;
    }
    this.queued += take;
  }

  popRaw() {
    if (this.queued === 0) {
      this.left = 0;
      this.right = 0;
      return;
    }
    this.left = this.ring[this.read * 2];
    this.right = this.ring[this.read * 2 + 1];
    this.read = this.read + 1 === this.capacity ? 0 : this.read + 1;
    this.queued--;
  }

  nextFrame() {
    this.popRaw();
    if (this.blendPos < this.fade) {
      const oldLeft = this.blend[this.blendPos * 2];
      const oldRight = this.blend[this.blendPos * 2 + 1];
      this.blendPos++;
      const w = this.blendPos / (this.fade + 1);
      this.left = oldLeft + (this.left - oldLeft) * w;
      this.right = oldRight + (this.right - oldRight) * w;
    }
  }

  skipTo(keep) {
    const excess = this.queued - keep;
    if (excess <= this.fade) {
      return;
    }
    for (let k = 0; k < this.fade; k++) {
      this.popRaw();
      this.blend[k * 2] = this.left;
      this.blend[k * 2 + 1] = this.right;
    }
    const drop = excess - this.fade;
    this.read = (this.read + drop) % this.capacity;
    this.queued -= drop;
    this.blendPos = 0;
    this.overruns++;
    this.overrunFrames += excess;
  }

  process(inputs, outputs) {
    const out = outputs[0];
    const left = out[0];
    const right = out.length > 1 ? out[1] : null;
    const frames = left.length;

    if (!this.playing) {
      if (this.queued < this.target) {
        left.fill(0);
        if (right) {
          right.fill(0);
        }
        if (this.started && !this.paused) {
          this.underrunFrames += frames;
        }
        this.report();
        return true;
      }
      this.playing = true;
      this.started = true;
      this.fadeIn = 0;
    }

    if (this.blendPos >= this.fade && this.queued > this.excess) {
      this.skipTo(this.target);
    }

    const starving = this.queued < frames + this.fade;
    const play = starving ? Math.min(this.queued, frames) : frames;
    const fadeOut = Math.max(Math.min(play, this.fade), 1);
    const step = 1 / this.fade;

    for (let i = 0; i < frames; i++) {
      let l = 0;
      let r = 0;
      if (i < play) {
        this.nextFrame();
        l = this.left;
        r = this.right;
        this.min = Math.min(this.min, l, r);
        this.max = Math.max(this.max, l, r);
      }
      let envelope = this.fadeIn;
      if (this.fadeIn < 1) {
        this.fadeIn = Math.min(1, this.fadeIn + step);
      }
      if (starving) {
        envelope *= Math.min((play - Math.min(i, play)) / fadeOut, 1);
      }
      if (right) {
        left[i] = l * envelope;
        right[i] = r * envelope;
      } else {
        left[i] = (l + r) * 0.5 * envelope;
      }
    }

    if (starving) {
      this.playing = false;
      if (!this.paused) {
        this.underruns++;
        this.underrunFrames += frames - play;
      }
    }

    this.report();
    return true;
  }

  report() {
    this.ticks++;
    if (this.ticks % 8 !== 0) {
      return;
    }
    this.port.postMessage({
      time: currentTime,
      queued: this.queued,
      received: this.received,
      playing: this.playing,
      underruns: this.underruns,
      underrunFrames: this.underrunFrames,
      overruns: this.overruns,
      overrunFrames: this.overrunFrames,
      min: this.min,
      max: this.max,
    });
  }
}

registerProcessor("citrine-output", CitrineOutput);
