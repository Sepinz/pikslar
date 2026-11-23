// The repository's links. The Pages workflow writes the repository ("owner/name") into the
// pikslar-repository meta tag when it builds the site; until then, or when the page is opened
// from disk, the links that need it are hidden and a note says the source is not public yet.
// Nothing else on the site needs script.
(function () {
  var meta = document.querySelector('meta[name="pikslar-repository"]');
  var repo = meta ? meta.getAttribute("content") : "";
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repo)) {
    document.documentElement.setAttribute("data-repo-missing", "");
    return;
  }
  var base = "https://github.com/" + repo;
  document.querySelectorAll("[data-repo-path]").forEach(function (a) {
    a.setAttribute("href", base + a.getAttribute("data-repo-path"));
  });
})();
