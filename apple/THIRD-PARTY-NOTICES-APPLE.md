# Third-party notices: the Apple TV and iPhone apps

These apps are built from the same source as Aurora for Windows and carry the same
licence: the **GNU General Public License, version 3 or later**. This page covers what
is different about the Apple builds. The general notices follow it, under "Third-party
notices". Where they mention `libmpv-2.dll`, read this page instead.

## libmpv and FFmpeg: the part that decodes the video

On Apple the video engine is **libmpv** with **FFmpeg**, taken from
[MPVKit](https://github.com/mpvkit/MPVKit) at version `1.1.0-n9.0.2`. The app uses
MPVKit's `MPVKit` product, its LGPL build of mpv and FFmpeg. It does not use the
`MPVKit-GPL` product, which enables GPL-only parts.

libmpv and FFmpeg in that configuration are licensed under the **GNU Lesser General
Public License, version 2.1 or later**.

### The relinking obligation

The LGPL requires that you can replace the library with your own modified version.
These builds do not link libmpv or FFmpeg statically. Each one ships as a separate
dynamic framework inside the app, in its `Frameworks` folder:

`Libmpv`, `Libavcodec`, `Libavdevice`, `Libavfilter`, `Libavformat`, `Libavutil`,
`Libswresample` and `Libswscale`, together with the libraries they use: `Libass`,
`Libbluray`, `Libcrypto`, `Libssl`, `Libdav1d`, `Libdovi`, `Libfreetype`,
`Libfribidi`, `Libharfbuzz`, `Libplacebo`, `Libshaderc_combined` and `Libuavs3d`.

Each of those libraries keeps its own licence, which is published with its source.

To replace one, build the app from this repository (`apple/README.md`) with your own
framework in its place. The Rust core and the Swift app are GPL-3.0-or-later, and all
of their source is in this repository.

### Source

- mpv: https://github.com/mpv-player/mpv
- FFmpeg: https://ffmpeg.org
- MPVKit's build scripts, which record how every framework above was configured:
  https://github.com/mpvkit/MPVKit

## Fonts

The apps carry the two typefaces Aurora for Windows uses, as TrueType files rebuilt from
the same fontsource packages (`Shared/Fonts`): **Manrope** (Copyright 2019 The Manrope
Project Authors) and **Bricolage Grotesque** (Copyright 2022 The Bricolage Grotesque
Project Authors). Both are licensed under the **SIL Open Font License, version 1.1**,
whose text travels with them in `Shared/Fonts/*-OFL.txt`.

## Distribution

These builds are for installing on your own devices from source. The GPL and the App
Store's terms are widely considered incompatible. `docs/APPLE_PORT_AND_SYNC.md` §2.6
covers what to settle before distributing them any further.
