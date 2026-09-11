# mathnote

a simple terminal notebook for prose, proofs, and mathematical expressions. converts only explicit or unambiguous mathematics into canonical LaTeX, compiles a real document with embedded Tectonic, rasterizes its pdf with Hayro, and displays the page. All mathematical typography comes from LaTeX and Latin Modern Math.

Tectonic is embedded as a Rust library. No external TeX executable or PDF viewer is used at runtime. Tectonic still links to native text libraries, so the build host and resulting executable need compatible ICU, FreeType, Graphite2, and libpng libraries. On macOS, the repository Cargo configuration discovers an existing Homebrew ICU installation from either the Apple Silicon or Intel prefix. The produced macOS binary is host-local rather than a standalone relocatable bundle, so rebuild it on a destination with matching native libraries.

The first launch retrieves only the files required by mathnote's fixed TeX template from one pinned format-33 Tectonic bundle. Mathnote rejects any bundle whose cryptographic content digest differs from the compiled-in expected digest, then caches the verified resources under the platform cache directory. Later compilations lock Tectonic to cache-only operation and work offline. If that verified cache becomes unusable, mathnote invalidates it and makes one online repair attempt.
