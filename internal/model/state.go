package model

import "time"

type State struct {
	ID        string    `json:"state_id"`
	UserID    string    `json:"user_id"`
	Text      string    `json:"text"`
	CreatedAt time.Time `json:"created_at"`
	ExpiresAt time.Time `json:"expires_at"`
}

type CreateStateRequest struct {
	Text      string `json:"text"`
	ExpiresAt int64  `json:"expires_at"`
}

type StateResponse struct {
	ID        string `json:"state_id"`
	UserID    string `json:"user_id"`
	Text      string `json:"text"`
	CreatedAt int64  `json:"created_at"`
	ExpiresAt int64  `json:"expires_at"`
}

type LatestStateResponse struct {
	ID        string `json:"state_id"`
	UserID    string `json:"user_id"`
	Email     string `json:"email"`
	Text      string `json:"text"`
	CreatedAt int64  `json:"created_at"`
	ExpiresAt int64  `json:"expires_at"`
}

type PaginatedStatesResponse struct {
	Latest *StateResponse  `json:"latest"`
	Items  []StateResponse `json:"items"`
	Page   int             `json:"page"`
	Size   int             `json:"size"`
	Total  int             `json:"total"`
}
