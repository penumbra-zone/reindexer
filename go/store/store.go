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

type BlockResult int

const (
	BlockNotFound BlockResult = -1
	BlockTooBig   BlockResult = -2
)

func (s *Store) BlockByHeight(height int64, output []byte) (BlockResult, error) {
	block := s.db.LoadBlock(height)
	if block == nil {
		return BlockNotFound, nil
	}
	proto, err := block.ToProto()
	if err != nil {
		return 0, err
	}
  size := proto.Size()
	if size >= len(output) {
		return BlockTooBig, err
	}
	_, err = proto.MarshalTo(output)
	if err != nil {
		return 0, err
	}
	return BlockResult(size), nil
}
