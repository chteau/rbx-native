if (!matchMedia("(prefers-reduced-motion: reduce)").matches) document.documentElement.classList.add("motion");

document.querySelectorAll("table").forEach((table) => {
  const box = document.createElement("div");
  box.className = "tbl";
  table.replaceWith(box);
  box.append(table);
});

document.querySelectorAll("pre").forEach((pre) => {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "copy";
  button.textContent = "Copy";
  button.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(pre.querySelector("code").textContent);
      button.textContent = "Copied";
    } catch {
      button.textContent = "Select it";
    }
    setTimeout(() => { button.textContent = "Copy"; }, 1600);
  });
  pre.append(button);
});
