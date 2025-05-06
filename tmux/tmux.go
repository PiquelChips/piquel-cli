package tmux

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"slices"
	"strings"

	"github.com/PiquelChips/piquel-cli/config"
	"github.com/PiquelChips/piquel-cli/models"
	"github.com/PiquelChips/piquel-cli/utils"
)

func ListSessions(listConfig, listTmux bool) error {
	sessions := []string{}
	if listTmux {
		tmuxSessions, err := ListTmuxSessions()
		if err != nil {
			return err
		}
		sessions = append(sessions, tmuxSessions...)
	}

	if listConfig {
		for session := range config.Config.Sessions {
			sessions = append(sessions, session)
		}
	}

	slices.Sort(sessions)
	sessions = slices.Compact(sessions)

	for _, session := range sessions {
		fmt.Printf("%s\n", session)
	}

	return nil
}

func ListTmuxSessions() ([]string, error) {
	sessions, err := execTmuxReturn("list-sessions", "-F", "#{session_name}")
	if err != nil {
		if strings.HasPrefix(sessions, "no server running on") || strings.HasPrefix(sessions, "error connecting to") {
			return []string{}, nil
		}
		return []string{}, fmt.Errorf("Failed to list sessions with error: %s\n", sessions)
	}

	sessions = strings.Trim(sessions, "\n")
	return strings.Split(sessions, "\n"), nil
}

func Attach(session string) (string, error) {
	return execTmuxReturn("attach", "-t", session)
}

func NewSession(sessionName string, session *models.SessionConfig) error {
	if err := validateSession(sessionName, session); err != nil {
		return err
	}

	if err := execTmux("new-session", "-Ad", "-c", session.Root, "-s", sessionName); err != nil {
		return fmt.Errorf("Failed to create session with name %s\n", sessionName)
	}

	index, err := execTmuxReturn("list-windows", "-t", sessionName, "-F", "#{window_index}")
	if err != nil {
		return fmt.Errorf("Failed to list tmux windows with error: %s\n", index)
	}
	index = strings.Trim(index, "\n")

	for index, window := range session.Windows {
		if err := NewWindow(session.Root, window); err != nil {
			return fmt.Errorf("Failed to create windows %d with error: %s\n", index+1, err.Error())
		}
	}

	if err := execTmux("kill-window", "-t", sessionName+":"+index); err != nil {
		return fmt.Errorf("Failed to kill first window\n")
	}

	if err := execTmux("select-window", "-t", sessionName+":"+index); err != nil {
		return fmt.Errorf("Failed to select first window\n")
	}

	if result, err := Attach(sessionName); err != nil {
		return fmt.Errorf("Failed to attach to session with error: %s\n", result)
	}
	return nil
}

func NewWindow(startDir string, window *models.WindowConfig) error {
	if result, err := execTmuxReturn("new-window", "-c", startDir); err != nil {
		return fmt.Errorf("Failed to create window with error: %s\n", result)
	}

	for _, command := range window.Commands {
		if result, err := execTmuxReturn("send-keys", command, "Enter"); err != nil {
			return fmt.Errorf("Failed to execute command \"%s\" with error: %s\n", command, result)
		}
	}

	return nil
}

func execTmux(args ...string) error {
	command := exec.Command("tmux", args...)
	command.Stdin = os.Stdin
	return command.Run()
}

func execTmuxReturn(args ...string) (string, error) {
	command := exec.Command("tmux", args...)
	command.Stdin = os.Stdin
	result, err := command.CombinedOutput()
	return string(result), err
}

func validateSession(name string, session *models.SessionConfig) error {
	if strings.TrimSpace(name) == "" {
		return fmt.Errorf("\"%s\" is not valid session name", name)
	}

	session.Root = utils.ExpandHome(session.Root)
	if _, err := os.Stat(session.Root); config.Config.ValidateSessionRoot && errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("Path %s does not exist", session.Root)
	}

	if len(session.Windows) < 1 {
		return fmt.Errorf("Session must have at least one window")
	}
	return nil
}
