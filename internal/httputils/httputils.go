package httputils

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
)

func DecodeJSON(r *http.Request, v any) error {
	dec := json.NewDecoder(r.Body)
	dec.DisallowUnknownFields()
	if err := dec.Decode(v); err != nil {
		return err
	}
	// Require exactly one JSON value and consume trailing whitespace so body
	// limits also apply to padding after an otherwise valid request.
	var extra any
	if err := dec.Decode(&extra); err != io.EOF {
		if err != nil {
			return err
		}
		return errors.New("multiple JSON values")
	}
	return nil
}

func WriteJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

func WriteError(w http.ResponseWriter, status int, msg string) {
	WriteJSON(w, status, map[string]string{"error": msg})
}
