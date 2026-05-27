package crypto

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"errors"
	"io"
	"strings"
)

type EmailCipher struct {
	aead    cipher.AEAD
	hmacKey []byte
}

func NewEmailCipher(encryptionKey, hmacKey []byte) (*EmailCipher, error) {
	if len(encryptionKey) != 32 {
		return nil, errors.New("encryption key must be 32 bytes")
	}
	if len(hmacKey) == 0 {
		return nil, errors.New("hmac key must not be empty")
	}

	block, err := aes.NewCipher(encryptionKey)
	if err != nil {
		return nil, err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	return &EmailCipher{aead: gcm, hmacKey: hmacKey}, nil
}

func (c *EmailCipher) Encrypt(plain string) (string, error) {
	nonce := make([]byte, c.aead.NonceSize())
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return "", err
	}
	ct := c.aead.Seal(nonce, nonce, []byte(plain), nil)
	return base64.RawStdEncoding.EncodeToString(ct), nil
}

func (c *EmailCipher) Decrypt(encoded string) (string, error) {
	ct, err := base64.RawStdEncoding.DecodeString(encoded)
	if err != nil {
		return "", err
	}
	ns := c.aead.NonceSize()
	if len(ct) < ns {
		return "", errors.New("ciphertext too short")
	}
	nonce, body := ct[:ns], ct[ns:]
	plain, err := c.aead.Open(nil, nonce, body, nil)
	if err != nil {
		return "", err
	}
	return string(plain), nil
}

func (c *EmailCipher) Hash(plain string) string {
	mac := hmac.New(sha256.New, c.hmacKey)
	mac.Write([]byte(strings.ToLower(strings.TrimSpace(plain))))
	return base64.RawStdEncoding.EncodeToString(mac.Sum(nil))
}
