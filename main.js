// When a release has a Linux build, the link downloads it. Until then it
// points at the build instructions.
fetch("https://api.github.com/repos/chteau/rbx-native/releases?per_page=10")
  .then((reply) => (reply.ok ? reply.json() : []))
  .then((releases) => {
    for (const release of releases) {
      if (release.draft) continue;
      const asset = release.assets.find((a) => /x86_64-unknown-linux-gnu\.tar\.gz$/.test(a.name));
      if (!asset) continue;
      const link = document.getElementById("download");
      link.href = asset.browser_download_url;
      link.textContent = "Download for Linux";
      document.getElementById("download-note").textContent =
        release.tag_name + ", unpack it and run rbxstudio. macOS and Windows releases are upcoming, Linux goes first.";
      return;
    }
  })
  .catch(() => {});
