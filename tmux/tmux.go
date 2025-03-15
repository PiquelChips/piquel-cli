package tmux

import (
	"os"
	"os/exec"
	"strings"

	"github.com/PiquelChips/piquel-cli/config"
)

func ListSessions() ([]string, error) {
	listSessionsCommand := exec.Command("tmux", "list-sessions", "-F", "#{session_name}")

	sessionBytes, err := listSessionsCommand.Output()
	if err != nil {
		return []string{}, err
	}

	return strings.Split(string(sessionBytes), "\n"), nil
}

func Attach(session string) (string, error) {
	return execTmuxReturn("attach", "-t", session)
}

func NewSession(sessionName string, session *config.SessionConfig) error {
	if err := execTmux("new-session", "-Ad", "-c", session.Root, "-s", sessionName); err != nil {
		return err
	}

	index, err := execTmuxReturn("list-windows", "-t", sessionName, "-F", "#{window_index}")
	if err != nil {
		return err
	}
	index = strings.Trim(index, "\n")

	for _, window := range session.Windows {
		if err := NewWindow(session.Root, window); err != nil {
			return err
		}
	}

	if err := execTmux("kill-window", "-t", sessionName+":"+index); err != nil {
		return err
	}

	if err := execTmux("select-window", "-t", sessionName+":"+index); err != nil {
		return err
	}

	_, err = Attach(sessionName)
	return err
}

func NewWindow(startDir string, window *config.WindowConfig) error {
	err := execTmux("new-window", "-c", startDir)
	if err != nil {
		return err
	}

	for _, command := range window.Commands {
		if err := execTmux("send-keys", command, "Enter"); err != nil {
			return err
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
