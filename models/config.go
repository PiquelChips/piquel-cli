package models

type PiquelConfig struct {
	ValidateSessionRoot bool                      `yaml:"validate_session_root"`
	Sessions            map[string]*SessionConfig `yaml:"sessions"`
	DefaultSession      []*WindowConfig           `yaml:"default_session"`
}

type SessionConfig struct {
	Root    string          `yaml:"root"`
	Windows []*WindowConfig `yaml:"windows"`
}

type WindowConfig struct {
	Commands []string `yaml:"commands"`
}
