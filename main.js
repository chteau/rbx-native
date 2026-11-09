// Sections fade in the first time they scroll into view.
const reveal = document.querySelectorAll(".reveal");
if ("IntersectionObserver" in window) {
  const seen = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (entry.isIntersecting) {
        entry.target.classList.add("seen");
        seen.unobserve(entry.target);
      }
    }
  }, { rootMargin: "0px 0px -8% 0px" });
  reveal.forEach((el) => seen.observe(el));
} else {
  reveal.forEach((el) => el.classList.add("seen"));
}

// Copy buttons on the code blocks.
document.querySelectorAll("[data-copy]").forEach((button) => {
  button.addEventListener("click", async () => {
    const text = document.getElementById(button.dataset.copy).textContent;
    try {
      await navigator.clipboard.writeText(text);
      button.textContent = "Copied";
    } catch {
      button.textContent = "Select it";
    }
    setTimeout(() => { button.textContent = "Copy"; }, 1600);
  });
});

// Once a release has a Linux build, the main button downloads it. Until then
// it stays on the build-from-source instructions, so it's never a dead link.
(async () => {
  try {
    const reply = await fetch("https://api.github.com/repos/chteau/rbx-native/releases?per_page=10");
    if (!reply.ok) return;
    for (const release of await reply.json()) {
      if (release.draft) continue;
      const asset = release.assets.find((a) => /x86_64-unknown-linux-gnu\.tar\.gz$/.test(a.name));
      if (!asset) continue;
      const button = document.getElementById("download");
      button.href = asset.browser_download_url;
      button.textContent = "Download for Linux";
      document.getElementById("download-note").textContent =
        release.tag_name + (release.prerelease ? ", pre-release" : "") + ". Unpack it and run rbxstudio.";
      return;
    }
  } catch {
    // Offline or rate limited: the build instructions are still right there.
  }
})();
