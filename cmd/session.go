package cmd

import (
	"fmt"
	"os"
	"strings"

	"github.com/PiquelChips/piquel-cli/config"
	"github.com/PiquelChips/piquel-cli/models"
	"github.com/PiquelChips/piquel-cli/tmux"
	"github.com/spf13/cobra"
)

var sessionCmd = &cobra.Command{
	Use:     "session",
	Short:   "Creates a session with default session config",
	Aliases: []string{"s"},
	Args:    cobra.RangeArgs(0, 1),
	RunE: func(cmd *cobra.Command, args []string) error {
		if _, ok := os.LookupEnv("TMUX"); ok {
			return fmt.Errorf("Please do not use this command in tmux")
		}

		sessionConfig := &models.SessionConfig{}
		sessionConfig.Windows = config.Config.DefaultSession

		if len(args) == 1 {
			sessionConfig.Root = args[0]
		}

		nameSplit := strings.Split(sessionConfig.Root, "/")
		return tmux.NewSession(strings.ToLower(nameSplit[len(nameSplit)-1]), sessionConfig)
	},
}

func init() {
	rootCmd.AddCommand(sessionCmd)
}
