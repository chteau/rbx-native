rbx-native alpha test build
===========================

Thanks for testing! This is a build straight from CI. The Windows and Linux
builds are unsigned, and the macOS one is signed with RbxNative's own
certificate but not notarized by Apple, so your OS will be suspicious of it
the first time. On Windows and Linux it is a single program, nothing to
install: put it in any folder and run it from there.

Opening it
----------

Windows
  Double-click RbxNative.exe. If SmartScreen says "Windows protected your
  PC", click "More info", then "Run anyway". Keep WinDivert.dll and
  WinDivert64.sys next to it: Play's network simulator needs them
  (WinDivert is LGPLv3, see WinDivert-LICENSE.txt).

macOS (Apple Silicon)
  Open the .dmg and drag RbxNative onto Applications, replacing the one
  already there. macOS quarantines downloads that Apple hasn't notarized,
  so the first time, right-click RbxNative in Applications, choose Open,
  then Open again. Or, in a Terminal:
    xattr -dr com.apple.quarantine /Applications/RbxNative.app
    open /Applications/RbxNative.app

Linux (x86_64, X11)
  It needs the X11/xkbcommon/fontconfig libraries and a Vulkan driver.
  On Debian/Ubuntu:
    sudo apt install libxkbcommon-x11-0 libx11-xcb1 libfontconfig1 libvulkan1 mesa-vulkan-drivers
  Then:
    chmod +x rbxnative
    ./rbxnative
  Open/Save As dialogs go through xdg-desktop-portal, and the API key is
  kept in the Secret Service (GNOME Keyring, KWallet); most desktops
  already run both.

First launch
------------

The first launch opens a setup wizard that asks for a Roblox Open Cloud API
key (it is stored in your OS keyring, never in a plain file). You can skip
it and still open and edit local places; publishing and cloud features need
the key. Run `rbxnative --setup` to go through the wizard again.
`rbxnative --help` lists the other options.

Feedback
--------

Bugs, crashes and ideas: https://github.com/chteau/rbx-native/issues
Please say which OS you are on and include the line below.
