# Lone Echo OpenXR

Play **Lone Echo** and **Lone Echo II** on Linux or Windows using OpenXR. You need your own copies of the games; no game files are included.

## Why this exists

This project aims to keep the Lone Echo games playable beyond the life of their original, neglected VR platform. Development began on Linux, the developer’s primary system. Windows support was added to make the games accessible to more players and easier to keep playing in the years ahead.

## Getting started

1. Download the ZIP from the [latest release](https://github.com/morekitsch/lone-echo-openxr/releases/latest) and extract it somewhere you want to keep it.
2. Copy the **complete contents of each game's installation folder** to the matching destination below.

   | Game | Destination |
   | --- | --- |
   | Lone Echo | `games/lone-echo/` |
   | Lone Echo II | `games/lone-echo-2/` |

   The `_data` and `bin` folders should sit directly inside each destination folder. You can install either game or both.

3. Run the installer using the instructions below.
4. Connect your headset, then launch the game using its new shortcut.

### Windows

- Use 64-bit Windows with either Virtual Desktop and VDXR or SteamVR.
- Double-click **`setup.exe`**. Python is included.
- Choose **Install a game** and follow the prompts.
- Select `vdxr` for Virtual Desktop or `steamvr` for SteamVR.
- Connect your headset through your chosen runtime.
- Launch the game using its Desktop or Start menu shortcut.

### Linux

- Install WiVRn or SteamVR, GE-Proton with OpenXR support, and Python 3.10 or newer.
- Open a terminal in the extracted folder and run:

  ```sh
  bash setup.sh
  ```

- Choose **Install a game** and follow the prompts.
- Select `wivrn` or `steamvr`.
- Connect your headset through your chosen runtime.
- Launch the game from your applications menu. WiVRn also lists the installed games.

## Updating or uninstalling

Use **Uninstall** in the setup menu before updating or removing the package. Keep `games` and, on Linux, `userdata` to preserve your games and saves.

For other setup options and troubleshooting, see the [setup reference](SETUP.md). See the [release notes](RELEASE_NOTES.md) for known issues and testing status. To build from source, follow the [build instructions](source/BUILDING.md).

## Contributors

- **[morekitsch](https://github.com/morekitsch)** — Project creator and maintainer.
- **OpenAI Codex** — AI assistance with development, debugging, testing, and documentation.

This project is based on [libovr-openxr-rs](https://github.com/TesseractCat/libovr-openxr-rs). [License](LICENSE.txt) · [Attribution](source/runtime/UPSTREAM.md)
