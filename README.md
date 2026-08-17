A Batch encoder using FFmpeg

## Building BFFmpeg

### Requirements

- A recent stable version of [Rust](https://www.rust-lang.org/tools/install)
- FFmpeg and FFprobe available on your system `PATH`
- Git

Verify the required tools:

```text
rustc --version
cargo --version
ffmpeg -version
ffprobe -version
```

Some included presets use NVIDIA NVENC. These require a supported NVIDIA GPU, current drivers, and an FFmpeg build with NVENC enabled. CPU-based x264 and x265 presets do not require NVIDIA hardware.

### Clone the repository

```bash
git clone <repository-url>
cd encodef
```

Replace `<repository-url>` with the URL of the BFFmpeg repository.

### Windows

Install Rust using `rustup`, then install an FFmpeg build and add its `bin` directory to your system `PATH`.

Build a release executable:

```powershell
cargo build --release
```

The executable will be created at:

```text
target\release\bffmpeg.exe
```

Copy the configuration file beside the executable:

```powershell
Copy-Item config.json target\release\config.json
```

Run BFFmpeg:

```powershell
.\target\release\bffmpeg.exe
```

For a portable distribution, place these files together in the same folder:

```text
BFFmpeg/
├── bffmpeg.exe
└── config.json
```

### Linux

Install the compiler, common desktop dependencies, and FFmpeg. On Debian or Ubuntu:

```bash
sudo apt update
sudo apt install build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev libssl-dev ffmpeg
```

Install Rust if it is not already installed:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Build BFFmpeg:

```bash
cargo build --release
```

Copy the configuration beside the executable:

```bash
cp config.json target/release/config.json
```

Run it:

```bash
./target/release/bffmpeg
```

The packaged directory should contain:

```text
BFFmpeg/
├── bffmpeg
└── config.json
```

### Development build

To compile and run BFFmpeg directly from the repository:

```bash
cargo run
```

During development, BFFmpeg will find `config.json` in the repository working directory. Packaged builds should keep `config.json` beside the executable.

### Tests

Run the automated test suite with:

```bash
cargo test
```

Check the project without producing a complete executable:

```bash
cargo check
```

### Configuration and permissions

BFFmpeg saves user-created presets back to `config.json`. The application directory must therefore be writable.

Avoid installing BFFmpeg and its configuration directly under protected locations such as `C:\Program Files` unless configuration storage is changed to a user-writable directory.

### Troubleshooting

If BFFmpeg cannot find FFmpeg, confirm these commands work from the same terminal used to launch the application:

```bash
ffmpeg -version
ffprobe -version
```

On Windows, check whether the installed FFmpeg build supports NVIDIA encoding:

```powershell
ffmpeg -encoders | Select-String nvenc
```

On Linux:

```bash
ffmpeg -encoders | grep nvenc
```

If NVENC is unavailable, select one of the software x264 or x265 presets instead.
