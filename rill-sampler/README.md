# rill-sampler

Sample playback and time-series reading nodes for the Rill signal graph.

## Components

- **`SamplePlayerNode`** — stereo sample-playback source node. Supports
  one-shot, forward-loop, and ping-pong loop modes. All parameters
  automatable via patchbay.
- **`SampleBuffer`** — sample container for mono/stereo audio data with
  metadata (sample rate, name).
- **`TimeSeriesNode`** — multi-channel source node for unevenly-sampled
  time series. Uses binary-search + interpolation (nearest/linear/cubic).
- **`TimeSeriesReader`** — stand-alone reader for time-series data.
- **`load_wav()`** — load 16-bit PCM WAV files into `SampleBuffer`
  (feature `"wav"`, enabled by default).
- **`from_csv()`** — load time-series data from CSV string
  (`t,channel,value` format).

## Usage

```rust
use rill_sampler::buffer::SampleBuffer;
use rill_sampler::player::SamplePlayerNode;

// Load a WAV file
let sample = rill_sampler::wav::load_wav("kick.wav").unwrap();

// Create a sample-player node (mono output port 0, stereo adds port 1)
let mut player = SamplePlayerNode::<f32, 256>::new();
player.load(sample);
player.play();
```

## Feature flags

| Feature | Description | Default |
|---------|-------------|---------|
| `wav` | WAV file loading via `hound` | yes |

## Dependencies

- `rill-core` — signal node traits, `Port`, `ProcessResult`
- `rill-core-dsp` — `SamplePlayer`, `InterpolatedReader`, `Interpolate` trait

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-sampler>
