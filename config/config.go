package config

import (
	"fmt"
	"os"

	"github.com/PiquelChips/piquel-cli/models"
	"gopkg.in/yaml.v3"
)

var Config models.PiquelConfig
var configLoaded bool = false

func LoadConfig(configPath string) error {
	if configLoaded {
		return fmt.Errorf("Config has already been loaded from %s", configPath)
	}

	configFile, err := os.ReadFile(configPath)
	if err != nil {
		return fmt.Errorf("Config file %s does not exist", configPath)
	}

	yaml.Unmarshal(configFile, &Config)

	configLoaded = true
	return nil
}
