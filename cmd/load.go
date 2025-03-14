package cmd

import (
	"fmt"
	"os"
	"os/exec"
	"strings"

	"github.com/PiquelChips/piquel-cli/config"
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

		listSessionsCommand := exec.Command("tmux", "list-sessions")
		sessionBytes, err := listSessionsCommand.Output()
		if err != nil {
			return err
		}
		sessions := string(sessionBytes)
		if strings.Contains(sessions, session+":") {
			resultBytes, err := exec.Command("tmux", "attach", "-t", session).CombinedOutput()
			result := string(resultBytes)
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

		fmt.Printf("%v", sessionConfig)

		return nil
	},
}

func init() {
	rootCmd.AddCommand(loadCmd)
}
