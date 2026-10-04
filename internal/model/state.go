package model

import "time"

type State struct {
	ID        string    `json:"state_id"`
	UserID    string    `json:"user_id"`
	Text      string    `json:"text"`
	CreatedAt time.Time `json:"created_at"`
	ExpiresAt time.Time `json:"expires_at"`
}

// StateFilter narrows a state lookup. It flows from the handler down to the
// repository. Email is the optional client-supplied filter; UserID is resolved
// from it server-side and is what the repository actually queries on. An empty
// filter matches every state.
type StateFilter struct {
	Email  string
	UserID string
}

type CreateStateRequest struct {
	// Text is untrusted plain text, not HTML. Markup is preserved, not sanitized.
	Text      string `json:"text"`
	ExpiresAt int64  `json:"expires_at"`
}

type StateResponse struct {
	ID     string `json:"state_id"`
	UserID string `json:"user_id"`
	// Text must be rendered as text (e.g. textContent), never as raw HTML.
	// JSON escaping does not make the decoded value safe for HTML insertion.
	Text      string `json:"text"`
	CreatedAt int64  `json:"created_at"`
	ExpiresAt int64  `json:"expires_at"`
}

type LatestStateResponse struct {
	ID     string `json:"state_id"`
	UserID string `json:"user_id"`
	Email  string `json:"email"`
	// Text has the same untrusted plain-text contract as StateResponse.Text.
	Text      string `json:"text"`
	CreatedAt int64  `json:"created_at"`
	ExpiresAt int64  `json:"expires_at"`
}

type PaginatedStatesResponse struct {
	Items []StateResponse `json:"items"`
	Page  int             `json:"page"`
	Size  int             `json:"size"`
	Total int             `json:"total"`
}
