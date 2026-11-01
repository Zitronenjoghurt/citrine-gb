const VIDEO_CODECS = ["avc1.42E01F", "avc1.4D401F", "avc1.640028"];
const AUDIO_CODECS = ["mp4a.40.2", "opus"];

function videoConfig(codec, width, height, fps, bitrate) {
  return {
    codec,
    width,
    height,
    framerate: fps,
    bitrate,
    bitrateMode: "variable",
    avc: { format: "avc" },
  };
}

function audioConfig(codec, sampleRate, bitrate) {
  return { codec, sampleRate, numberOfChannels: 2, bitrate };
}

export async function probeRecording(width, height, fps, sampleRate, videoBitrate, audioBitrate) {
  if (typeof VideoEncoder === "undefined" || typeof AudioEncoder === "undefined") {
    return { video: "", audio: "", reason: "This browser can't record video. Chrome and Safari can." };
  }
  let video = "";
  for (const codec of VIDEO_CODECS) {
    try {
      const result = await VideoEncoder.isConfigSupported(videoConfig(codec, width, height, fps, videoBitrate));
      if (result.supported) {
        video = codec;
        break;
      }
    } catch (_) {}
  }
  let audio = "";
  for (const codec of AUDIO_CODECS) {
    try {
      const result = await AudioEncoder.isConfigSupported(audioConfig(codec, sampleRate, audioBitrate));
      if (result.supported) {
        audio = codec;
        break;
      }
    } catch (_) {}
  }
  let reason = "";
  if (!video) {
    reason = "This browser can't make MP4 video. Chrome and Safari can.";
  } else if (!audio) {
    reason = "This browser can't record the sound. Chrome and Safari can.";
  }
  return { video, audio, reason };
}

export class WebRecorder {
  constructor(videoCodec, audioCodec, width, height, fps, sampleRate, videoBitrate, audioBitrate, onVideo, onAudio, onError) {
    this.chunks = [];
    this.width = width;
    this.height = height;
    this.sampleRate = sampleRate;
    this.failed = false;
    const fail = (error) => {
      if (!this.failed) {
        this.failed = true;
        onError(String(error && error.message ? error.message : error));
      }
    };
    this.video = new VideoEncoder({
      output: (chunk, meta) => {
        const data = new Uint8Array(chunk.byteLength);
        chunk.copyTo(data);
        this.chunks.push(data);
        const description = meta && meta.decoderConfig && meta.decoderConfig.description;
        onVideo(data.byteLength, chunk.type === "key", description ? new Uint8Array(description) : null);
      },
      error: fail,
    });
    this.audio = new AudioEncoder({
      output: (chunk, meta) => {
        const data = new Uint8Array(chunk.byteLength);
        chunk.copyTo(data);
        this.chunks.push(data);
        const description = meta && meta.decoderConfig && meta.decoderConfig.description;
        onAudio(data.byteLength, chunk.duration || 0, description ? new Uint8Array(description) : null);
      },
      error: fail,
    });
    try {
      this.video.configure(videoConfig(videoCodec, width, height, fps, videoBitrate));
      this.audio.configure(audioConfig(audioCodec, sampleRate, audioBitrate));
    } catch (error) {
      fail(error);
    }
  }

  backlog() {
    return this.video.encodeQueueSize + this.audio.encodeQueueSize;
  }

  frame(rgba, timestamp, duration, keyFrame) {
    if (this.failed) {
      return;
    }
    const frame = new VideoFrame(rgba, {
      format: "RGBA",
      codedWidth: this.width,
      codedHeight: this.height,
      timestamp,
      duration,
    });
    this.video.encode(frame, { keyFrame });
    frame.close();
  }

  sound(samples, timestamp) {
    if (this.failed || samples.length < 2) {
      return;
    }
    const data = new AudioData({
      format: "f32",
      sampleRate: this.sampleRate,
      numberOfFrames: samples.length / 2,
      numberOfChannels: 2,
      timestamp,
      data: samples,
    });
    this.audio.encode(data);
    data.close();
  }

  async finish() {
    if (!this.failed) {
      await Promise.all([this.video.flush(), this.audio.flush()]);
    }
    this.video.close();
    this.audio.close();
    return !this.failed;
  }

  blob(head, moov) {
    const blob = new Blob([head, ...this.chunks, moov], { type: "video/mp4" });
    this.chunks = [];
    return blob;
  }
}
