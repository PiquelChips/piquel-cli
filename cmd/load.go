package cmd

import (
	"fmt"
	"os"
	"slices"
	"strings"

	"github.com/PiquelChips/piquel-cli/config"
	"github.com/PiquelChips/piquel-cli/tmux"
	"github.com/spf13/cobra"
)

var loadCmd = &cobra.Command{
	Use:     "load session",
	Short:   "Loads a tmux session from config or connects to existing one",
	Aliases: []string{"l"},
	Args:    cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		if _, ok := os.LookupEnv("TMUX"); ok {
			return fmt.Errorf("Please do not use this command in tmux")
		}

		session := args[0]

		sessions, err := tmux.ListSessions(false)
		if err != nil {
			return err
		}

		if slices.Contains(sessions, session) {
			result, err := tmux.Attach(session)
			if err == nil {
				return nil
			} else if !strings.HasPrefix(result, "can't find session:") {
				return fmt.Errorf(result)
			}
		}

		sessionConfig, ok := config.Config.Sessions[session]
		if !ok {
			return fmt.Errorf("Invalid session")
		}

		return tmux.NewSession(session, sessionConfig)
	},
}

func init() {
	rootCmd.AddCommand(loadCmd)
}
