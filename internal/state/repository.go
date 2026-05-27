package state

import (
	"context"
	"database/sql"

	cerr "statesu.com/internal/error"
	"statesu.com/internal/model"
)

type Repository struct {
	db *sql.DB
}

func NewRepository(db *sql.DB) *Repository {
	return &Repository{db: db}
}

func (r *Repository) Create(ctx context.Context, s model.State) error {
	_, err := r.db.ExecContext(ctx,
		`INSERT INTO state (id, user_id, text, created_at, expires_at) VALUES (?, ?, ?, ?, ?)`,
		s.ID, s.UserID, s.Text, s.CreatedAt, s.ExpiresAt,
	)
	return err
}

func (r *Repository) GetLatest(ctx context.Context) (model.State, string, error) {
	var s model.State
	var encEmail string
	err := r.db.QueryRowContext(ctx,
		`SELECT s.id, s.user_id, s.text, s.created_at, s.expires_at, u.email
		 FROM state s
		 JOIN user u ON u.id = s.user_id
		 ORDER BY s.created_at DESC LIMIT 1`,
	).Scan(&s.ID, &s.UserID, &s.Text, &s.CreatedAt, &s.ExpiresAt, &encEmail)
	if err == sql.ErrNoRows {
		return model.State{}, "", cerr.ErrStateNotFound
	}
	if err != nil {
		return model.State{}, "", err
	}
	return s, encEmail, nil
}

func (r *Repository) ListByUserID(ctx context.Context, userID string, limit, offset int) ([]model.State, error) {
	rows, err := r.db.QueryContext(ctx,
		`SELECT id, user_id, text, created_at, expires_at
		 FROM state
		 WHERE user_id = ?
		 ORDER BY created_at DESC
		 LIMIT ? OFFSET ?`,
		userID, limit, offset,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var states []model.State
	for rows.Next() {
		var s model.State
		if err := rows.Scan(&s.ID, &s.UserID, &s.Text, &s.CreatedAt, &s.ExpiresAt); err != nil {
			return nil, err
		}
		states = append(states, s)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	return states, nil
}

func (r *Repository) CountByUserID(ctx context.Context, userID string) (int, error) {
	var n int
	err := r.db.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM state WHERE user_id = ?`,
		userID,
	).Scan(&n)
	return n, err
}

func (r *Repository) GetLatestByUserID(ctx context.Context, userID string) (model.State, error) {
	var s model.State
	err := r.db.QueryRowContext(ctx,
		`SELECT id, user_id, text, created_at, expires_at
		 FROM state
		 WHERE user_id = ?
		 ORDER BY created_at DESC
		 LIMIT 1`,
		userID,
	).Scan(&s.ID, &s.UserID, &s.Text, &s.CreatedAt, &s.ExpiresAt)
	if err == sql.ErrNoRows {
		return model.State{}, cerr.ErrStateNotFound
	}
	if err != nil {
		return model.State{}, err
	}
	return s, nil
}

func (r *Repository) GetByID(ctx context.Context, id string) (model.State, error) {
	var s model.State
	err := r.db.QueryRowContext(ctx,
		`SELECT id, user_id, text, created_at, expires_at
		 FROM state
		 WHERE id = ?`,
		id,
	).Scan(&s.ID, &s.UserID, &s.Text, &s.CreatedAt, &s.ExpiresAt)
	if err == sql.ErrNoRows {
		return model.State{}, cerr.ErrStateNotFound
	}
	if err != nil {
		return model.State{}, err
	}
	return s, nil
}

func (r *Repository) DeleteByID(ctx context.Context, stateID, userID string) error {
	res, err := r.db.ExecContext(ctx, `DELETE FROM state WHERE id = ? AND user_id = ?`, stateID, userID)
	if err != nil {
		return err
	}
	n, err := res.RowsAffected()
	if err != nil {
		return err
	}
	if n == 0 {
		return cerr.ErrStateNotFound
	}
	return nil
}
