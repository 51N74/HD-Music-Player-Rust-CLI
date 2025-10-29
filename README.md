<<<<<<< HEAD
# rMusic — High-Resolution CLI Audio Player (Rust)

rMusic is a high-performance command-line audio player implemented in Rust. It focuses on accurate playback of high-resolution audio files, robust queue and playlist management, and a small, scriptable CLI surface. The project compiles to a native binary (`rmusic`) and includes examples and tests.

This README covers building, running, testing, and the available CLI commands.

---

## Quick summary

- Language: Rust (edition 2021)
- Binary: `rmusic` (declared in `Cargo.toml`)
- Purpose: CLI audio player with support for common lossless and lossy formats, queue and playlist management, device selection, and interactive mode.
- Example code and demos: `examples/config_demo.rs`

---

## Requirements

- Rust toolchain (rustc + cargo). Stable or recent nightly recommended (toolchain compatible with the dependencies declared in `Cargo.toml`; Rust 1.60+ is a reasonable minimum).
- System audio backend (the project uses `cpal` for audio output). On some platforms you may need to install audio-related system libraries or grant audio permissions.
- Optional: `ffmpeg` or other format tools are not required because decoding uses the `symphonia` crate.

---

## Build

From the repository root (the crate is the `rmusic` package), run:

- Debug build:
`cargo build`

- Release build:
`cargo build --release`

The `rmusic` binary will be built under `target/debug/` or `target/release/` respectively.

---

## Run

You can run the binary directly with `cargo run` or invoke the built binary.

- Run with cargo (debug):
`cargo run --bin rmusic -- <COMMAND>`

- Run built binary (release):
`./target/release/rmusic <COMMAND>`

Notes:
- When using `cargo run` and supplying arguments for the program, separate `--` between cargo arguments and program arguments (as shown above).
- The crate also defines an example `config_demo` that can be run with `cargo run --example config_demo`.

---

## CLI overview

The CLI is implemented with `clap` and supports the following top-level commands and subcommands. Use `rmusic help` or run the binary with no args to show the same help text.

Primary commands:
- `play [path]` — start playback. If `path` is a file or directory, it will be queued / played.
- `pause` — pause playback.
- `resume` — resume from pause.
- `stop` — stop playback and reset position.
- `next` — advance to next track in the queue.
- `prev` (alias `previous`) — go back to previous track.
- `seek <position>` — seek to a time in the current track. Formats accepted: `MM:SS`, `MM:SS.s`, `90`, `90s`, or decimals `30.5`.
- `status` — display current status and track metadata.
- `watch` — continuously update status (live view).
- `volume <0-100>` — set playback volume.

Queue subcommands (`queue <action>`):
- `queue add <path>` — add file/directory to the current queue.
- `queue list` — list queued tracks.
- `queue clear` — clear the queue.
- `queue position` — show current index in the queue.

Playlist subcommands (`playlist <action>`):
- `playlist save <name>` — save the current queue as a playlist.
- `playlist load <name>` — load a playlist into the queue.
- `playlist list` — list saved playlists.
- `playlist delete <name>` — delete a playlist.

Device subcommands (`device <action>`):
- `device list` — list available audio output devices.
- `device set <name|id>` — select a specific output device.

Interactive mode
- The binary can be run interactively (if implemented by current code flow) by starting it without a terminal command and typing commands at the prompt (e.g. `play`, `pause`, `queue add /path/to/file`, etc.). The CLI module also exposes a parser for commands typed in interactive mode.

Examples:
- Play a file:
`rmusic play /path/to/file.flac`

- Add a directory to the queue:
`rmusic queue add ~/Music/Album`

- Seek to 1 minute 30 seconds:
`rmusic seek 1:30`

- List devices:
`rmusic device list`

---

## Configuration

- The project provides a configuration module. If the project ships a `.env.example` or config example, copy it to create your local config (for example `cp .env.example .env`) and edit as needed.
- Playlists and persistent configuration (device selection, saved playlists) are typically stored in a user data directory (see the `dirs` crate usage). Check the `examples` folder and the `config` module for exact locations.

---

## Tests

Run the test suite with:

`cargo test`

The repository contains unit and integration tests under `src/` and `src/audio/tests`. Running `cargo test` executes both unit and integration tests configured by the crate.

---

## Concrete examples (repo-specific)

Below are concrete commands you can run from the repository root. These use files and targets that exist in this repo (for example, the `config_demo` example and the `rmusic` binary defined in `Cargo.toml`).

Build the project (debug and release):
```/dev/null/build-commands.sh#L1-2
# debug build
cargo build

# release build (recommended for playback performance)
cargo build --release
```

Run the CLI help (shows current commands implemented in `src/cli/mod.rs`):
```/dev/null/help.sh#L1
cargo run --bin rmusic -- --help
```

Run the included example (`examples/config_demo.rs`):
```/dev/null/examples.sh#L1
cargo run --example config_demo
```

Add the repository `examples` directory to the player's queue and start playback (interactive/audio system permitting). This demonstrates the `queue add` and `play` commands implemented in the CLI:
```/dev/null/cli-example.sh#L1-2
cargo run --bin rmusic -- queue add examples
cargo run --bin rmusic -- play
```

List the current queue (after adding items):
```/dev/null/cli-queue-list.sh#L1
cargo run --bin rmusic -- queue list
```

Run the test suite (unit + integration tests present under `src/` and `src/audio/tests/`):
```/dev/null/test-commands.sh#L1
cargo test
```

Run a single unit test from the `cli` module (example test names live in `src/cli/mod.rs` tests):
```/dev/null/test-single.sh#L1
cargo test cli::path_tests::test_expand_path_tilde_home
```

Run the release binary directly (after `cargo build --release`):
```/dev/null/run-release.sh#L1
./target/release/rmusic --help
```

Enumerate audio output devices and set one by name (device commands are in `src/cli/mod.rs` and device handling in `src/audio/device.rs`):
```/dev/null/device-list.sh#L1-2
cargo run --bin rmusic -- device list
cargo run --bin rmusic -- device set \"Built-in Output\"
```

Notes:
- The `queue add` command accepts relative paths and expands `~` to the user home (see `CliApp::expand_path` in `src/cli/mod.rs`).
- Decoding is handled by the decoders in `src/audio/decoders/`, so `queue add` will attempt to enqueue recognized audio files recursively from the provided directory.
- If you plan to do real playback, prefer the release build for better performance: `cargo build --release` then run `./target/release/rmusic play <path>`.
- Use `RUST_LOG=debug` to see more diagnostic messages via the `log` + `env_logger` setup (e.g. `RUST_LOG=debug cargo run --bin rmusic -- status`).

---

## Logging & debugging

- The project uses `log` + `env_logger` for logging. To see debug or trace logs, set the `RUST_LOG` environment variable when running, for example:
`RUST_LOG=debug cargo run --bin rmusic -- status`

- When debugging audio issues, check device selection (`device list`) and system audio permissions. For low-level decoding issues review `symphonia` messages in the logs.

---

## Development notes

- Concurrency and async: the project uses `tokio` for asynchronous tasks.
- Audio output: implemented via `cpal`. Device support and sample formats are handled in the `audio` module.
- Decoding: `symphonia` handles many audio formats (FLAC, ALAC, MP3, OGG, WAV, M4A, etc.). See `src/audio/decoders` for format-specific code.
- Error handling uses `thiserror` and the project exposes structured `PlayerError` types.

---

## Contributing

1. Fork the repository and create a feature branch:
   - `git checkout -b feat/your-feature`
2. Write code and tests.
3. Run tests and linter locally (`cargo test` and any formatter / linter you prefer).
4. Open a pull request with a clear description of changes and motivation.

Please avoid committing secrets (API keys) or local environment files.

---

## Troubleshooting

- Build failures:
  - Ensure your Rust toolchain is up to date: `rustup update`.
  - Some platform-specific audio backends or headers may be required; consult `cpal` docs for your platform.
- Audio device not found:
  - Use `rmusic device list` to enumerate system output devices and `rmusic device set <name>` to select one.
- Playback stuttering:
  - Try a release build (`cargo build --release`).
  - Check CPU / scheduling and other processes using audio hardware.

---
=======
# HD-Music-Player-Rust-CLI
>>>>>>> 24dc3c9853de85eb274a59ee09aee1b05be0db47
