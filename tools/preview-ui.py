"""Local UI preview and navigation regression tests. Does not serve keys/build files."""
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
ROUTES = {
    "pc": ROOT / "build/dayplanner-pc/app/ui",
    "android": ROOT / "build/dayplanner-android/app/ui",
    "tests": ROOT / "tests",
}


class PreviewHandler(SimpleHTTPRequestHandler):
    def translate_path(self, path):
        parts = unquote(urlsplit(path).path).strip("/").split("/")
        base = ROUTES.get(parts[0])
        if base is None or any(part in ("..", ".") or "\\" in part or ":" in part for part in parts):
            return str(ROOT / "__not_served__")
        candidate = base.joinpath(*parts[1:]).resolve()
        if not candidate.is_relative_to(base.resolve()):
            return str(ROOT / "__not_served__")
        return str(candidate)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


if __name__ == "__main__":
    print("UI: http://127.0.0.1:8788/pc/ or /android/", flush=True)
    print("Tests: http://127.0.0.1:8788/tests/navigation.html", flush=True)
    ThreadingHTTPServer(("127.0.0.1", 8788), PreviewHandler).serve_forever()
