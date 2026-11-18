// One button per system, the visitor's own first and filled in. Each gets the
// newest release's file for it, best kind first. If GitHub doesn't answer they
// stay on the install guide's section for that system.
const systems = {
  windows: { name: "Windows", kinds: [/-windows-x64-setup\.exe$/] },
  macos: { name: "macOS", kinds: [/-macos-arm64\.dmg$/] },
  linux: { name: "Linux", kinds: [/-x86_64\.AppImage$/, /x86_64-unknown-linux-gnu\.tar\.gz$/] },
};
const mine = /Windows/.test(navigator.userAgent)
  ? "windows"
  : /Macintosh|Mac OS X/.test(navigator.userAgent)
    ? "macos"
    : /Linux|X11/.test(navigator.userAgent) && !/Android/.test(navigator.userAgent)
      ? "linux"
      : null;
const row = document.getElementById("downloads");
const buttons = [...row.querySelectorAll("a[data-os]")];
if (mine) {
  for (const button of buttons) {
    const own = button.dataset.os === mine;
    button.className = own ? "primary" : "secondary";
    button.textContent = own ? "Download for " + systems[mine].name : systems[button.dataset.os].name;
  }
  row.prepend(buttons.find((b) => b.dataset.os === mine), " ");
}
fetch("https://api.github.com/repos/chteau/rbx-native/releases?per_page=10")
  .then((reply) => (reply.ok ? reply.json() : []))
  .then((releases) => {
    let tag = null;
    for (const button of buttons) {
      const { kinds } = systems[button.dataset.os];
      for (const release of releases) {
        if (release.draft) continue;
        const kind = kinds.find((k) => release.assets.some((a) => k.test(a.name)));
        if (!kind) continue;
        button.href = release.assets.find((a) => kind.test(a.name)).browser_download_url;
        if (button.dataset.os === mine || !tag) tag = release.tag_name;
        break;
      }
    }
    if (tag) {
      document.getElementById("download-note").innerHTML =
        tag + ". Each system has its own quirks, they're in the <a href=\"docs/install.html#systems\">install guide</a>.";
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
