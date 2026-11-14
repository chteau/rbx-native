// The newest release with a build for this visitor's system gets the direct
// link, best kind first. If GitHub doesn't answer the page still points at
// releases/latest.
const system = /Windows/.test(navigator.userAgent)
  ? { name: "Windows", kinds: [/-windows-x64-setup\.exe$/] }
  : /Macintosh|Mac OS X/.test(navigator.userAgent)
    ? { name: "macOS", kinds: [/-macos-arm64\.dmg$/] }
    : { name: "Linux", kinds: [/-x86_64\.AppImage$/, /x86_64-unknown-linux-gnu\.tar\.gz$/] };
fetch("https://api.github.com/repos/chteau/rbx-native/releases?per_page=10")
  .then((reply) => (reply.ok ? reply.json() : []))
  .then((releases) => {
    for (const release of releases) {
      if (release.draft) continue;
      const kind = system.kinds.find((k) => release.assets.some((a) => k.test(a.name)));
      if (!kind) continue;
      const asset = release.assets.find((a) => kind.test(a.name));
      const button = document.getElementById("download");
      button.href = asset.browser_download_url;
      button.textContent = "Download for " + system.name;
      document.getElementById("download-note").innerHTML =
        release.tag_name + ". Other systems and the one-line install are in the <a href=\"docs/install.html\">install guide</a>.";
      return;
    }
  })
  .catch(() => {});

fetch("https://api.github.com/repos/chteau/rbx-native")
  .then((reply) => (reply.ok ? reply.json() : null))
  .then((repo) => {
    if (repo && repo.stargazers_count) document.getElementById("stars").textContent = " \u2605 " + repo.stargazers_count;
  })
  .catch(() => {});

const nav = document.getElementById("nav");
const moving = document.documentElement.classList.contains("motion");
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));

if (moving) {
  // Reveal once, as things come on screen.
  const seen = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting) {
        e.target.classList.add("in");
        seen.unobserve(e.target);
      }
    }
  }, { rootMargin: "0px 0px -8% 0px" });
  document.querySelectorAll(".reveal").forEach((el) => seen.observe(el));
}

// Clips only run while they're on screen. With reduced motion they stay on the
// poster and get normal controls.
const clips = [...document.querySelectorAll("video[data-clip]")];
if (moving) {
  const watcher = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting) e.target.play().catch(() => {});
      else e.target.pause();
    }
  }, { threshold: 0.35 });
  clips.forEach((clip) => watcher.observe(clip));
} else {
  clips.forEach((clip) => { clip.controls = true; });
}

const showcase = document.querySelector("#showcase img");

let last = scrollY;
let queued = false;

function frame() {
  queued = false;
  const y = scrollY;
  const vh = innerHeight;

  nav.classList.toggle("solid", y > 40);
  nav.classList.toggle("away", y > 400 && y > last);
  last = y;

  if (!moving) return;

  const top = showcase.parentElement.getBoundingClientRect().top;
  const grown = clamp(1 - top / (vh * 0.85), 0, 1);
  showcase.style.setProperty("--grow", (0.84 + grown * 0.16).toFixed(4));

}

addEventListener("scroll", () => {
  if (!queued) {
    queued = true;
    requestAnimationFrame(frame);
  }
}, { passive: true });
addEventListener("resize", frame);
frame();
