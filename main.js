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
      document.getElementById("download-note").textContent =
        release.tag_name + ", unpack it and run rbxstudio. macOS and Windows releases are upcoming, Linux goes first.";
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

const showcase = document.querySelector("#showcase img");
const drifters = [...document.querySelectorAll("[data-drift]")];

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

  if (innerWidth > 860) {
    for (const el of drifters) {
      const box = el.getBoundingClientRect();
      if (box.bottom < -200 || box.top > vh + 200) continue;
      const off = (box.top + box.height / 2 - vh / 2) / vh;
      el.style.translate = `0 ${(off * Number(el.dataset.drift)).toFixed(1)}px`;
    }
  }
}

addEventListener("scroll", () => {
  if (!queued) {
    queued = true;
    requestAnimationFrame(frame);
  }
}, { passive: true });
addEventListener("resize", frame);
frame();
