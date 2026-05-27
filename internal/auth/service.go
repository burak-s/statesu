package auth

import (
	"context"
	"errors"

	"github.com/google/uuid"
	"modernc.org/sqlite"
	sqlite3 "modernc.org/sqlite/lib"

	"statesu.com/internal/crypto"
	cerr "statesu.com/internal/error"
	"statesu.com/internal/model"
)

type userRepository interface {
	Create(ctx context.Context, u model.User) error
	GetByEmailHash(ctx context.Context, hash string) (model.User, error)
}

type emailCipher interface {
	Encrypt(plain string) (string, error)
	Decrypt(encoded string) (string, error)
	Hash(plain string) string
}

type Service struct {
	repo   userRepository
	cipher emailCipher
}

func NewService(repo userRepository, cipher emailCipher) *Service {
	return &Service{repo: repo, cipher: cipher}
}

func (s *Service) Register(ctx context.Context, email, password string) (model.User, error) {
	email = normalizeEmail(email)
	if err := validateEmail(email); err != nil {
		return model.User{}, err
	}
	if err := validatePassword(password); err != nil {
		return model.User{}, err
	}

	hash, err := crypto.HashPassword(password)
	if err != nil {
		return model.User{}, err
	}

	encEmail, err := s.cipher.Encrypt(email)
	if err != nil {
		return model.User{}, err
	}

	u := model.User{
		ID:        uuid.NewString(),
		Email:     encEmail,
		EmailHash: s.cipher.Hash(email),
		Password:  hash,
	}

	if err := s.repo.Create(ctx, u); err != nil {
		var serr *sqlite.Error
		if errors.As(err, &serr) && serr.Code() == sqlite3.SQLITE_CONSTRAINT_UNIQUE {
			return model.User{}, cerr.ErrUserExists
		}
		return model.User{}, err
	}

	u.Email = email
	return u, nil
}

func (s *Service) Login(ctx context.Context, email, password string) (model.User, error) {
	email = normalizeEmail(email)
	if email == "" || password == "" {
		return model.User{}, cerr.ErrInvalidCredentials
	}

	u, err := s.repo.GetByEmailHash(ctx, s.cipher.Hash(email))
	if err != nil {
		if errors.Is(err, cerr.ErrUserNotFound) {
			return model.User{}, cerr.ErrInvalidCredentials
		}
		return model.User{}, err
	}

	if err := crypto.CheckPassword(u.Password, password); err != nil {
		return model.User{}, cerr.ErrInvalidCredentials
	}

	plain, err := s.cipher.Decrypt(u.Email)
	if err != nil {
		return model.User{}, err
	}
	u.Email = plain
	return u, nil
}
