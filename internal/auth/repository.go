package auth

import (
	"context"
	"database/sql"
	"errors"

	cerr "statesu.com/internal/error"
	"statesu.com/internal/model"
)

type Repository struct {
	db *sql.DB
}

func NewRepository(db *sql.DB) *Repository {
	return &Repository{db: db}
}

func (r *Repository) Create(ctx context.Context, u model.User) error {
	_, err := r.db.ExecContext(ctx,
		`INSERT INTO user (id, email, email_hash, password) VALUES (?, ?, ?, ?)`,
		u.ID, u.Email, u.EmailHash, u.Password,
	)
	return err
}

func (r *Repository) GetByID(ctx context.Context, id string) (model.User, error) {
	var u model.User
	err := r.db.QueryRowContext(ctx,
		`SELECT id, email, email_hash, password FROM user WHERE id = ?`,
		id,
	).Scan(&u.ID, &u.Email, &u.EmailHash, &u.Password)
	if errors.Is(err, sql.ErrNoRows) {
		return model.User{}, cerr.ErrUserNotFound
	}
	return u, err
}

func (r *Repository) GetByEmailHash(ctx context.Context, hash string) (model.User, error) {
	var u model.User
	err := r.db.QueryRowContext(ctx,
		`SELECT id, email, email_hash, password FROM user WHERE email_hash = ?`,
		hash,
	).Scan(&u.ID, &u.Email, &u.EmailHash, &u.Password)
	if errors.Is(err, sql.ErrNoRows) {
		return model.User{}, cerr.ErrUserNotFound
	}
	return u, err
}

func (r *Repository) Update(ctx context.Context, u model.User) error {
	res, err := r.db.ExecContext(ctx,
		`UPDATE user SET email = ?, email_hash = ?, password = ? WHERE id = ?`,
		u.Email, u.EmailHash, u.Password, u.ID,
	)
	if err != nil {
		return err
	}
	n, err := res.RowsAffected()
	if err != nil {
		return err
	}
	if n == 0 {
		return cerr.ErrUserNotFound
	}
	return nil
}

func (r *Repository) Delete(ctx context.Context, id string) error {
	res, err := r.db.ExecContext(ctx, `DELETE FROM user WHERE id = ?`, id)
	if err != nil {
		return err
	}
	n, err := res.RowsAffected()
	if err != nil {
		return err
	}
	if n == 0 {
		return cerr.ErrUserNotFound
	}
	return nil
}
