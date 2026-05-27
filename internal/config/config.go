package config

import (
	"encoding/base64"
	"fmt"
	"os"
)

type Config struct {
	Addr            string
	DBPath          string
	EmailEncryptKey []byte
	EmailHMACKey    []byte
	JWTSecret       []byte
}

func Load() (Config, error) {
	encKey, err := decodeKey("STATESU_EMAIL_KEY", 32)
	if err != nil {
		return Config{}, err
	}
	hmacKey, err := decodeKey("STATESU_EMAIL_HMAC_KEY", 0)
	if err != nil {
		return Config{}, err
	}
	jwtSecret, err := decodeKey("STATESU_JWT_SECRET", 0)
	if err != nil {
		return Config{}, err
	}

	return Config{
		Addr:            getenv("STATESU_ADDR", ":8080"),
		DBPath:          getenv("STATESU_DB", "stateu.db"),
		EmailEncryptKey: encKey,
		EmailHMACKey:    hmacKey,
		JWTSecret:       jwtSecret,
	}, nil
}

func getenv(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func decodeKey(env string, requiredLen int) ([]byte, error) {
	v := os.Getenv(env)
	if v == "" {
		return nil, fmt.Errorf("%s is required", env)
	}
	b, err := base64.StdEncoding.DecodeString(v)
	if err != nil {
		return nil, fmt.Errorf("%s: invalid base64: %w", env, err)
	}
	if requiredLen > 0 && len(b) != requiredLen {
		return nil, fmt.Errorf("%s must decode to %d bytes, got %d", env, requiredLen, len(b))
	}
	return b, nil
}
