package store

import (
	db "github.com/cometbft/cometbft-db"
	"github.com/cometbft/cometbft/store"
)

const DATABASE_NAME = "blockstore"

type Store struct {
	db *store.BlockStore
}

func NewStore(backend string, dir string) (*Store, error) {
	db, err := db.NewDB(DATABASE_NAME, db.BackendType(backend), dir)
	if err != nil {
		return nil, err
	}

	return &Store{
		db: store.NewBlockStore(db),
	}, nil
}

func (s *Store) Height() int64 {
	return s.db.Height()
}
