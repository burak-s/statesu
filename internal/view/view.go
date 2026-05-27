package view

import (
	"bytes"
	"embed"
	"fmt"
	"html/template"
	"io/fs"
	"net/http"
	"path/filepath"
	"strings"
)

//go:embed templates/*
var templateFS embed.FS

//go:embed static/*
var staticFS embed.FS

type Renderer struct {
	base  *template.Template
	pages map[string]*template.Template
}

func New() (*Renderer, error) {
	base, err := template.ParseFS(templateFS,
		"templates/layouts/*.html",
		"templates/partials/*.html",
	)
	if err != nil {
		return nil, fmt.Errorf("parse base templates: %w", err)
	}

	pageFiles, err := fs.Glob(templateFS, "templates/pages/*.html")
	if err != nil {
		return nil, fmt.Errorf("glob pages: %w", err)
	}

	pages := make(map[string]*template.Template, len(pageFiles))
	for _, path := range pageFiles {
		name := strings.TrimSuffix(filepath.Base(path), ".html")
		clone, err := base.Clone()
		if err != nil {
			return nil, fmt.Errorf("clone base for %s: %w", name, err)
		}
		if _, err := clone.ParseFS(templateFS, path); err != nil {
			return nil, fmt.Errorf("parse page %s: %w", name, err)
		}
		pages[name] = clone
	}

	return &Renderer{base: base, pages: pages}, nil
}

func (r *Renderer) Page(w http.ResponseWriter, name string, data any) {
	t, ok := r.pages[name]
	if !ok {
		http.Error(w, "unknown page: "+name, http.StatusInternalServerError)
		return
	}
	r.execute(w, t, "base", data)
}

func (r *Renderer) Partial(w http.ResponseWriter, name string, data any) {
	r.execute(w, r.base, name, data)
}

func (r *Renderer) Auto(w http.ResponseWriter, req *http.Request, page, partial string, data any) {
	if req.Header.Get("HX-Request") == "true" {
		r.Partial(w, partial, data)
		return
	}
	r.Page(w, page, data)
}

func (r *Renderer) execute(w http.ResponseWriter, t *template.Template, name string, data any) {
	var buf bytes.Buffer
	if err := t.ExecuteTemplate(&buf, name, data); err != nil {
		http.Error(w, "template error", http.StatusInternalServerError)
		return
	}
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	_, _ = buf.WriteTo(w)
}

func (r *Renderer) MountStatic(mux *http.ServeMux) {
	sub, err := fs.Sub(staticFS, "static")
	if err != nil {
		panic(err)
	}
	mux.Handle("GET /static/", http.StripPrefix("/static/", http.FileServerFS(sub)))
}
