Upstream notice files for the release package
=============================================

The release script (spikes/make-release.ps1) copies these files, unchanged,
into THIRD-PARTY-NOTICES.txt. It refuses to build a package for distribution
until all three are here. Each is the exact file from the named upstream source, fetched unchanged
and saved here under the name given, never retyped.

  COPYING.MinGW-w64-runtime.txt
      mingw-w64, tag v14.0.0:
      COPYING.MinGW-w64-runtime/COPYING.MinGW-w64-runtime.txt
      (the runtime linked into both program files; see section 2)

  aseprite-doc-LICENSE.txt
      Aseprite, commit 9cf3c128: src/doc/LICENSE.txt
      (the MIT licence of the code the blending follows; see section 4)

  pixman-COPYING.txt
      pixman (freedesktop.org): COPYING
      (saved from the aseprite/pixman mirror, commit 285b9a90, which is
      the pixman Aseprite builds with)
      (MUL_UN8 and DIV_UN8; see section 4)

Saved as UTF-8 or ASCII; line endings do not matter. The release script runs without -AllowIncompleteNotices.
