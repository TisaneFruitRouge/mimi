// The only script on the site: a Copy button for the install command.
for (const button of document.querySelectorAll("[data-copy]")) {
  const source = document.getElementById(button.dataset.copy);
  const label = button.querySelector("span");
  if (!source || !navigator.clipboard) continue;
  button.hidden = false;
  let timer;
  button.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(source.textContent.trim());
      label.textContent = "Copied";
    } catch {
      label.textContent = "Select and copy";
    }
    clearTimeout(timer);
    timer = setTimeout(() => (label.textContent = "Copy"), 2000);
  });
}
