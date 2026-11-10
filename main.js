// Newest release with a Linux build gets the direct link. If GitHub doesn't
// answer the page still points at releases/latest.
fetch("https://api.github.com/repos/chteau/rbx-native/releases?per_page=10")
  .then((reply) => (reply.ok ? reply.json() : []))
  .then((releases) => {
    for (const release of releases) {
      if (release.draft) continue;
      const asset = release.assets.find((a) => /x86_64-unknown-linux-gnu\.tar\.gz$/.test(a.name));
      if (!asset) continue;
      document.getElementById("download").href = asset.browser_download_url;
      document.getElementById("menu-download").href = asset.browser_download_url;
      document.getElementById("download-note").textContent =
        release.tag_name + ", unpack it and run rbxstudio. macOS and Windows releases are upcoming, Linux goes first.";
      return;
    }
  })
  .catch(() => {});

// Explorer rows: open a folded section before jumping to it, and keep the
// selection on whatever section is being read.
const rows = [...document.querySelectorAll(".tree a")];
const sections = rows.map((row) => document.querySelector(row.getAttribute("href")));

rows.forEach((row, i) => {
  row.addEventListener("click", () => {
    if (sections[i].tagName === "DETAILS") sections[i].open = true;
  });
});

let queued = false;
function select() {
  queued = false;
  const atBottom = innerHeight + scrollY >= document.documentElement.scrollHeight - 2;
  let current = 0;
  sections.forEach((section, i) => {
    if (section.getBoundingClientRect().top <= 120) current = i;
  });
  if (atBottom) current = sections.length - 1;
  rows.forEach((row, i) => row.classList.toggle("selected", i === current));
}
addEventListener("scroll", () => {
  if (!queued) { queued = true; requestAnimationFrame(select); }
}, { passive: true });
select();
