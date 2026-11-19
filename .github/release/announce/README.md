# Release announcements

A minor or major release (`v0.3.0`, `v1.0.0`, `v1.1.0`) also gets a post in
Discord #announcements that pings Release ping and links to its #releases
post. A patch (`v0.2.1`) or a pre-release only gets the #releases post, with
no ping. `discord-release.yml` does both.

To dress up the announcement, add a folder named after the tag here on dev
before you push the tag:

```
.github/release/announce/v0.3.0/
  post.md         what to say, Discord markdown, under ~1700 characters
  1-graph.png     screenshots, shown in name order (up to 10, 10 MB each)
  2-terrain.png
```

Without `post.md` the post lists the release's headlines (the bold first
sentence of each changelog entry). Without images it's just text.

Preview it without posting: `.github/release/announce.sh --dry-run release.json <link>`
(after `gh release view <tag> --json name,tagName,url,body > release.json`).
