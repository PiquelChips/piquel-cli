package config

import (
	"errors"
	"fmt"
	"os"

	"github.com/PiquelChips/piquel-cli/utils"
	"gopkg.in/yaml.v3"
)

var Config PiquelConfig
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

	for name, session := range Config.Sessions {
		session.Root = utils.ExpandHome(session.Root)
		if _, err := os.Stat(session.Root); errors.Is(err, os.ErrNotExist) {
			return fmt.Errorf("Path %s of session %s does not exist: ", session.Root, name)
		}
	}

	configLoaded = true
	return nil
}
