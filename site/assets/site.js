// The site's only script: a Copy button for the install command, and the download
// button pointed at this computer's installer. Without it, both links still work.

// Copy button.
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

// Download button: the installer for this computer, when the page can tell which.
const LATEST = "https://github.com/TisaneFruitRouge/mimi/releases/latest/download/";
const download = document.getElementById("download");
const note = document.getElementById("download-note");

function point(file, label, os, extra) {
  download.href = LATEST + file;
  download.querySelector("span").textContent = label;
  for (const a of document.querySelectorAll(`.packages a[data-os="${os}"]`)) a.classList.add("yours");
  if (extra) {
    note.replaceChildren(...extra);
    note.hidden = false;
  }
}

function link(text, href) {
  const a = document.createElement("a");
  a.href = href;
  a.textContent = text;
  return a;
}

async function pickDownload() {
  if (!download) return;
  const ua = navigator.userAgent;
  const platform = navigator.userAgentData?.platform ?? navigator.platform ?? "";
  // Phones and tablets say "Mac" or "Linux" too; Mimi doesn't run there. (An iPad
  // passes for a Mac, except that it has a touchscreen.)
  if (/iPhone|iPad|Android/i.test(ua)) return;
  if (/Mac/i.test(platform) && navigator.maxTouchPoints <= 1) {
    // Browsers built on Chromium can tell Apple silicon from Intel; Safari and Firefox
    // always say "Intel", so Apple silicon (every Mac since late 2020) is the default.
    let arch = "";
    try {
      arch = (await navigator.userAgentData?.getHighEntropyValues(["architecture"]))?.architecture ?? "";
    } catch {
      // Not available: keep the default.
    }
    if (arch === "x86") {
      point("Mimi_x64.dmg", "Download for Mac", "mac-intel", [
        "For Macs with Intel. ",
        link("Mac with Apple silicon?", LATEST + "Mimi_aarch64.dmg"),
      ]);
    } else {
      point("Mimi_aarch64.dmg", "Download for Mac", "mac-arm", [
        "For Macs with Apple silicon (M1 and later). ",
        link("Mac with Intel?", LATEST + "Mimi_x64.dmg"),
      ]);
    }
  } else if (/Linux/i.test(platform)) {
    point("Mimi_amd64.AppImage", "Download for Linux", "linux", [
      "An AppImage, which runs on any distribution. ",
      link("Prefer a .deb or .rpm?", "#install"),
    ]);
  }
}
pickDownload();
