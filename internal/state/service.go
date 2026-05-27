package state

import (
	"context"
	"errors"
	"strings"
	"time"

	"github.com/google/uuid"

	cerr "statesu.com/internal/error"
	"statesu.com/internal/model"
)

const (
	maxTextLen      = 4096
	maxTTL          = 30 * 24 * time.Hour
	defaultPageSize = 20
	maxPageSize     = 100
)

type stateRepository interface {
	Create(ctx context.Context, s model.State) error
	ListByUserID(ctx context.Context, userID string, limit, offset int) ([]model.State, error)
	CountByUserID(ctx context.Context, userID string) (int, error)
	GetLatestByUserID(ctx context.Context, userID string) (model.State, error)
	GetLatest(ctx context.Context) (model.State, string, error)
	GetByID(ctx context.Context, id string) (model.State, error)
	DeleteByID(ctx context.Context, stateID, userID string) error
}

type ListResult struct {
	Items  []model.State
	Latest *model.State
	Page   int
	Size   int
	Total  int
}

type emailCipher interface {
	Decrypt(encoded string) (string, error)
	Hash(plain string) string
}

type userLookup interface {
	GetByEmailHash(ctx context.Context, hash string) (model.User, error)
}

type Service struct {
	repo   stateRepository
	users  userLookup
	cipher emailCipher
	now    func() time.Time
}

func NewService(repo stateRepository, users userLookup, cipher emailCipher) *Service {
	return &Service{repo: repo, users: users, cipher: cipher, now: time.Now}
}

func (s *Service) Create(ctx context.Context, userID, text string, expiresAt time.Time) (model.State, error) {
	if userID == "" {
		return model.State{}, cerr.ErrInvalidInput
	}
	text = strings.TrimSpace(text)
	if text == "" || len(text) > maxTextLen {
		return model.State{}, cerr.ErrInvalidInput
	}

	now := s.now().UTC()
	expiresAt = expiresAt.UTC()
	if !expiresAt.After(now) || expiresAt.Sub(now) > maxTTL {
		return model.State{}, cerr.ErrInvalidInput
	}

	st := model.State{
		ID:        uuid.NewString(),
		UserID:    userID,
		Text:      text,
		CreatedAt: now,
		ExpiresAt: expiresAt,
	}
	if err := s.repo.Create(ctx, st); err != nil {
		return model.State{}, err
	}
	return st, nil
}

func (s *Service) List(ctx context.Context, email string, page, size int) (ListResult, error) {
	email = strings.TrimSpace(email)
	if email == "" {
		return ListResult{}, cerr.ErrInvalidInput
	}
	if page < 1 {
		page = 1
	}
	if size < 1 {
		size = defaultPageSize
	}
	if size > maxPageSize {
		size = maxPageSize
	}

	u, err := s.users.GetByEmailHash(ctx, s.cipher.Hash(email))
	if err != nil {
		if errors.Is(err, cerr.ErrUserNotFound) {
			return ListResult{Page: page, Size: size}, nil
		}
		return ListResult{}, err
	}
	userID := u.ID

	total, err := s.repo.CountByUserID(ctx, userID)
	if err != nil {
		return ListResult{}, err
	}

	items, err := s.repo.ListByUserID(ctx, userID, size, (page-1)*size)
	if err != nil {
		return ListResult{}, err
	}

	var latest *model.State
	l, err := s.repo.GetLatestByUserID(ctx, userID)
	if err != nil && !errors.Is(err, cerr.ErrStateNotFound) {
		return ListResult{}, err
	}
	if err == nil {
		latest = &l
	}

	return ListResult{
		Items:  items,
		Latest: latest,
		Page:   page,
		Size:   size,
		Total:  total,
	}, nil
}

func (s *Service) Latest(ctx context.Context) (model.State, string, error) {
	st, encEmail, err := s.repo.GetLatest(ctx)
	if err != nil {
		return model.State{}, "", err
	}
	email, err := s.cipher.Decrypt(encEmail)
	if err != nil {
		return model.State{}, "", err
	}
	return st, email, nil
}

func (s *Service) Delete(ctx context.Context, stateID, userID string) error {
	if stateID == "" || userID == "" {
		return cerr.ErrInvalidInput
	}
	return s.repo.DeleteByID(ctx, stateID, userID)
}
