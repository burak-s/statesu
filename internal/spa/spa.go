// Package spa serves the static API documentation.
//
// The frontend is built by web/ (SvelteKit + adapter-static) and copied into
// dist before the Go binary is compiled, so everything ships as one binary.
package spa

import (
	"embed"
	"io/fs"
	"net/http"
	"strings"
)

//go:embed all:dist
var distFS embed.FS

const indexFile = "index.html"

// Handler serves the prerendered documentation and its embedded assets.
// Removed web-app routes and other unknown paths return 404.
func Handler() http.Handler {
	sub, err := fs.Sub(distFS, "dist")
	if err != nil {
		panic(err)
	}
	files := http.FileServerFS(sub)

	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if path := strings.TrimPrefix(r.URL.Path, "/"); path != "" {
			if f, err := sub.Open(path); err == nil {
				_ = f.Close()
				if strings.HasPrefix(r.URL.Path, "/_app/immutable/") {
					w.Header().Set("Cache-Control", "public, max-age=31536000, immutable")
				}
				files.ServeHTTP(w, r)
				return
			}
		}
		if r.URL.Path == "/" {
			serveIndex(w, r, sub)
			return
		}
		http.NotFound(w, r)
	})
}

func serveIndex(w http.ResponseWriter, r *http.Request, sub fs.FS) {
	data, err := fs.ReadFile(sub, indexFile)
	if err != nil {
		http.NotFound(w, r)
		return
	}
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Header().Set("Cache-Control", "no-cache")
	_, _ = w.Write(data)
}

// Mount registers the documentation as the GET catch-all. More specific API patterns
// registered on the same mux take precedence.
func Mount(mux *http.ServeMux) {
	mux.Handle("GET /", Handler())
}
