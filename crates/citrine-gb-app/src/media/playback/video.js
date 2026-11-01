const SOURCE_WIDTH = 160;
const SOURCE_HEIGHT = 144;

export class ClipPlayer {
  static canPlay() {
    return document.createElement("video").canPlayType('video/mp4; codecs="avc1.42E01E"') !== "";
  }

  constructor(bytes) {
    this.url = URL.createObjectURL(new Blob([bytes], { type: "video/mp4" }));
    this.video = document.createElement("video");
    this.video.playsInline = true;
    this.video.preload = "auto";
    this.state = 0;
    this.dirty = true;
    this.shown = -1;
    this.video.addEventListener("loadeddata", () => {
      this.state = 1;
      this.dirty = true;
    });
    this.video.addEventListener("seeked", () => {
      this.dirty = true;
    });
    this.video.addEventListener("error", () => {
      this.state = 2;
    });
    this.video.src = this.url;
    this.canvas = document.createElement("canvas");
    this.context = null;
  }

  status() {
    return this.state;
  }

  size() {
    const width = this.video.videoWidth;
    const height = this.video.videoHeight;
    const scale = Math.floor(width / SOURCE_WIDTH);
    if (scale > 0 && width === SOURCE_WIDTH * scale && height === SOURCE_HEIGHT * scale) {
      return [SOURCE_WIDTH, SOURCE_HEIGHT];
    }
    return [width, height];
  }

  width() {
    return this.size()[0];
  }

  height() {
    return this.size()[1];
  }

  frame() {
    const video = this.video;
    if (this.state !== 1 || video.videoWidth === 0) {
      return null;
    }
    const time = video.currentTime;
    if (!this.dirty && time === this.shown) {
      return null;
    }
    this.dirty = false;
    this.shown = time;
    const [width, height] = this.size();
    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width;
      this.canvas.height = height;
      this.context = null;
    }
    if (!this.context) {
      this.context = this.canvas.getContext("2d", { willReadFrequently: true });
      this.context.imageSmoothingEnabled = false;
    }
    this.context.drawImage(video, 0, 0, width, height);
    const data = this.context.getImageData(0, 0, width, height).data;
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  }

  play() {
    const started = this.video.play();
    if (started && started.catch) {
      started.catch(() => {});
    }
  }

  pause() {
    this.video.pause();
  }

  seek(seconds) {
    this.video.currentTime = seconds;
    this.dirty = true;
  }

  position() {
    return this.video.currentTime || 0;
  }

  length() {
    const length = this.video.duration;
    return Number.isFinite(length) ? length : 0;
  }

  playing() {
    return !this.video.paused && !this.video.ended;
  }

  setMuted(muted) {
    this.video.muted = muted;
  }

  setVolume(volume) {
    this.video.volume = Math.min(1, Math.max(0, volume));
  }

  close() {
    this.video.pause();
    this.video.removeAttribute("src");
    this.video.load();
    URL.revokeObjectURL(this.url);
  }
}
