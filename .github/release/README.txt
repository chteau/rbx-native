RbxNative
=========

A native Roblox Studio replacement. Nothing to install: unpack this folder
anywhere you can write to and run the program from there. It keeps itself
up to date (Settings > Files & recovery > Updates): a new release downloads
in the background, is checked against its signed checksums and runs from
the next launch.

Windows
  Double-click rbxstudio.exe. The build isn't signed with a paid
  certificate, so SmartScreen may say "Windows protected your PC": click
  "More info", then "Run anyway". Keep WinDivert.dll and WinDivert64.sys
  next to it: Play's network simulator needs them.

macOS (Apple Silicon)
  Move RbxNative.app to Applications. It is only ad-hoc signed, so the
  first time right-click it, choose Open, then Open again (or run
  `xattr -dr com.apple.quarantine /Applications/RbxNative.app`). Run it
  from Applications rather than straight from the download, or macOS keeps
  it in a read-only place and it can't update itself.

Linux (x86_64, X11)
  It needs the X11/xkbcommon/fontconfig libraries and a Vulkan driver.
  On Debian/Ubuntu:
    sudo apt install libxkbcommon-x11-0 libx11-xcb1 libfontconfig1 libvulkan1 mesa-vulkan-drivers
  Then run ./rbxstudio. Open/Save As go through xdg-desktop-portal and the
  API key is kept in the Secret Service (GNOME Keyring, KWallet).

The first launch asks for a Roblox Open Cloud API key (kept in your OS
keyring). You can skip it and still edit local places. `rbxstudio --help`
lists the options.

LICENSE.txt is RbxNative's license, THIRD-PARTY-LICENSES.html covers the
Rust crates built into it, and THIRD-PARTY-EMBEDDED.txt the programs it
carries inside (luau-lsp, cloudflared, slirp4netns). On Windows,
WinDivert-LICENSE.txt covers WinDivert (LGPLv3), shipped unmodified from
https://github.com/basil00/WinDivert.

Bugs and ideas: https://github.com/chteau/rbx-native/issues
