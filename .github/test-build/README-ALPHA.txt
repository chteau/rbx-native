rbx-native alpha test build
===========================

Thanks for testing! This is an unsigned build straight from CI, so your OS
will be suspicious of it the first time. It is a single program, nothing to
install: put it in any folder and run it from there.

Opening it
----------

Windows
  Double-click rbxstudio.exe. If SmartScreen says "Windows protected your
  PC", click "More info", then "Run anyway". Keep WinDivert.dll and
  WinDivert64.sys next to it: Play's network simulator needs them
  (WinDivert is LGPLv3, see WinDivert-LICENSE.txt).

macOS (Apple Silicon)
  The zip drops the executable bit and macOS quarantines downloads, so in a
  Terminal, in the folder you unzipped into:
    chmod +x rbxstudio
    xattr -dr com.apple.quarantine rbxstudio
    ./rbxstudio
  Or right-click rbxstudio in Finder, choose Open, then Open again.

Linux (x86_64, X11)
  It needs the X11/xkbcommon/fontconfig libraries and a Vulkan driver.
  On Debian/Ubuntu:
    sudo apt install libxkbcommon-x11-0 libx11-xcb1 libfontconfig1 libvulkan1 mesa-vulkan-drivers
  Then:
    chmod +x rbxstudio
    ./rbxstudio
  Open/Save As dialogs go through xdg-desktop-portal, and the API key is
  kept in the Secret Service (GNOME Keyring, KWallet); most desktops
  already run both.

First launch
------------

The first launch opens a setup wizard that asks for a Roblox Open Cloud API
key (it is stored in your OS keyring, never in a plain file). You can skip
it and still open and edit local places; publishing and cloud features need
the key. Run `rbxstudio --setup` to go through the wizard again.
`rbxstudio --help` lists the other options.

Feedback
--------

Bugs, crashes and ideas: https://github.com/chteau/rbx-native/issues
Please say which OS you are on and include the line below.
