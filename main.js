// One button per system, Linux first: it's what RbxNative was built for.
// Each opens a dialog with two ways in: a file from the newest release, or
// the one-line install. Without JS (or a dialog) the buttons stay links to
// the install guide's section for that system.
const systems = {
  linux: {
    name: "Linux",
    files: [
      [/-x86_64\.AppImage$/, "AppImage", "One file, runs on most distros. Make it executable and open it."],
      [/x86_64-unknown-linux-gnu\.tar\.gz$/, "Tarball", "Unpack anywhere and run rbxnative."],
    ],
    shell: "Terminal",
    line: "curl -fsSL https://rbxnative.chteau.bzh/install.sh | sh",
  },
  macos: {
    name: "macOS",
    files: [
      [/-macos-arm64\.dmg$/, "Disk image", "Apple silicon. Drag RbxNative to Applications."],
      [/aarch64-apple-darwin\.tar\.gz$/, "Tarball", "Apple silicon, the app bundle without the disk image."],
    ],
    shell: "Terminal",
    line: "curl -fsSL https://rbxnative.chteau.bzh/install.sh | sh",
  },
  windows: {
    name: "Windows",
    files: [
      [/-windows-x64-setup\.exe$/, "Installer", "64-bit. Installs for you only, no admin."],
      [/x86_64-pc-windows-msvc\.zip$/, "Portable zip", "Unzip anywhere and run rbxnative.exe."],
    ],
    shell: "PowerShell",
    line: "irm https://rbxnative.chteau.bzh/install.ps1 | iex",
  },
};
const found = {};
let tag = null;
let answered = false;
const sizeOf = (bytes) => (bytes / 1048576).toFixed(0) + " MB";

fetch("https://api.github.com/repos/chteau/rbx-native/releases?per_page=10")
  .then((reply) => (reply.ok ? reply.json() : []))
  .then((releases) => {
    answered = releases.length > 0;
    for (const [os, system] of Object.entries(systems)) {
      for (const release of releases) {
        if (release.draft) continue;
        const files = system.files
          .map(([kind, label, hint]) => [release.assets.find((a) => kind.test(a.name)), label, hint])
          .filter(([asset]) => asset);
        if (!files.length) continue;
        found[os] = { tag: release.tag_name, files };
        if (os === "linux" || !tag) tag = release.tag_name;
        break;
      }
    }
    if (tag) {
      document.getElementById("download-note").innerHTML =
        tag + ". Each system has its own quirks, they're in the <a href=\"docs/install.html#systems\">install guide</a>.";
    }
  })
  .catch(() => {});

const picker = document.getElementById("get");
const escape = (text) => text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

function open(os) {
  const system = systems[os];
  const release = found[os];
  picker.querySelector("h2").textContent = "RbxNative for " + system.name;
  picker.querySelector(".get-tag").textContent = release ? release.tag : "";
  picker.querySelector(".get-files").innerHTML = release
    ? release.files.map(([asset, label, hint]) =>
        `<a class="get-file" href="${escape(asset.browser_download_url)}"><span><strong>${label}</strong><small>${escape(hint)}</small></span><span class="get-size">${sizeOf(asset.size)}</span></a>`).join("")
    : `<a class="get-file" href="https://github.com/chteau/rbx-native/releases/latest"><span><strong>Releases page</strong><small>${answered ? `No ${system.name} build in the latest releases yet.` : `GitHub didn't answer, pick the file for ${system.name} there.`}</small></span></a>`;
  picker.querySelector(".get-shell").textContent = system.shell;
  picker.querySelector(".get-line code").textContent = system.line;
  picker.querySelector(".get-guide").href = "docs/install.html#" + os;
  picker.querySelector(".get-copy").textContent = "Copy";
  picker.showModal();
}

if (picker && picker.showModal) {
  for (const button of document.querySelectorAll("#downloads a[data-os]")) {
    button.addEventListener("click", (event) => {
      event.preventDefault();
      open(button.dataset.os);
    });
  }
  picker.addEventListener("click", (event) => {
    if (event.target === picker || event.target.closest(".get-close")) picker.close();
  });
  picker.querySelector(".get-copy").addEventListener("click", (event) => {
    const copy = event.currentTarget;
    navigator.clipboard.writeText(picker.querySelector(".get-line code").textContent).then(
      () => { copy.textContent = "Copied"; },
      () => { copy.textContent = "Select it"; },
    );
  });
}

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
