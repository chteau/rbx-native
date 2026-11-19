// The changelog page: GitHub's releases, read live so a new release shows up
// without rebuilding the site. GitHub renders and sanitizes the notes
// (the html media type gives `body_html`).
const list = document.getElementById("releases");
const fallback = '<p class="state">Couldn\'t reach GitHub. The releases are all on <a href="https://github.com/chteau/rbx-native/releases">GitHub</a>.</p>';

// x.y.0 is a feature release, anything else a patch.
function kind(tag) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)(-.+)?$/.exec(tag);
  if (!m) return "";
  if (m[4]) return '<span class="kind">Pre-release</span>';
  return m[3] === "0" ? '<span class="kind big">Feature release</span>' : '<span class="kind">Patch</span>';
}

function text(value) {
  const span = document.createElement("span");
  span.textContent = value;
  return span.innerHTML;
}

fetch("https://api.github.com/repos/chteau/rbx-native/releases?per_page=50", {
  headers: { Accept: "application/vnd.github.html+json" },
})
  .then((response) => (response.ok ? response.json() : Promise.reject(response.status)))
  .then((releases) => {
    releases = releases.filter((release) => !release.draft);
    if (releases.length === 0) {
      list.innerHTML = '<p class="state">No release yet.</p>';
      return;
    }
    list.innerHTML = releases
      .map((release, i) => {
        const date = new Date(release.published_at);
        return `<details class="release" id="${text(release.tag_name)}"${i === 0 ? " open" : ""}>
  <summary><h2>${text(release.name || release.tag_name)}</h2>${kind(release.tag_name)}<time datetime="${release.published_at}">${date.toLocaleDateString("en", { year: "numeric", month: "long", day: "numeric" })}</time></summary>
  <div class="notes">${release.body_html || "<p>No notes.</p>"}<a class="on-gh" href="${release.html_url}">Downloads and full notes on GitHub</a></div>
</details>`;
      })
      .join("");
    // A link to one release (changelog.html#v0.2.0) opens it.
    const target = location.hash && document.getElementById(decodeURIComponent(location.hash.slice(1)));
    if (target instanceof HTMLDetailsElement) {
      target.open = true;
      target.scrollIntoView();
    }
  })
  .catch(() => {
    list.innerHTML = fallback;
  });
