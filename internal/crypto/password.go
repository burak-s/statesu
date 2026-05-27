package crypto

import "golang.org/x/crypto/bcrypt"

const passwordCost = 12

func HashPassword(plain string) (string, error) {
	h, err := bcrypt.GenerateFromPassword([]byte(plain), passwordCost)
	if err != nil {
		return "", err
	}
	return string(h), nil
}

func CheckPassword(hash, plain string) error {
	return bcrypt.CompareHashAndPassword([]byte(hash), []byte(plain))
}
