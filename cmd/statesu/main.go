package main

import (
	"log"
	"net/http"

	"github.com/joho/godotenv"

	"statesu.com/internal/auth"
	"statesu.com/internal/config"
	"statesu.com/internal/crypto"
	"statesu.com/internal/middleware"
	"statesu.com/internal/spa"
	"statesu.com/internal/state"
)

func main() {
	_ = godotenv.Load()

	cfg, err := config.Load()
	if err != nil {
		log.Fatal(err)
	}

	db, err := config.OpenDB(cfg.DBPath)
	if err != nil {
		log.Fatal(err)
	}
	defer db.Close()

	emailCipher, err := crypto.NewEmailCipher(cfg.EmailEncryptKey, cfg.EmailHMACKey)
	if err != nil {
		log.Fatalf("email cipher: %v", err)
	}

	jwtIssuer, err := crypto.NewJWTIssuer(cfg.JWTSecret)
	if err != nil {
		log.Fatalf("jwt issuer: %v", err)
	}

	authRepo := auth.NewRepository(db)
	authSvc := auth.NewService(authRepo, emailCipher)
	authHandler := auth.NewHandler(authSvc, jwtIssuer)

	stateRepo := state.NewRepository(db)
	stateSvc := state.NewService(stateRepo, authRepo, emailCipher)
	stateHandler := state.NewHandler(stateSvc, jwtIssuer)

	mux := http.NewServeMux()
	authHandler.Mount(mux)
	stateHandler.Mount(mux)
	spa.Mount(mux)

	log.Printf("listening on %s", cfg.Addr)
	if err := http.ListenAndServe(cfg.Addr, middleware.CORS(mux)); err != nil {
		log.Fatal(err)
	}
}
