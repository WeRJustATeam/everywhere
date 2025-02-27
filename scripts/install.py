import subprocess
import sys

class Installer:
    def is_installed(self):
        raise NotImplementedError("Subclasses should implement this method")

    def prepare_rsc(self):
        if not self.is_installed():
            print(f"{self.__class__.__name__} is not installed.")
        else:
            print(f"{self.__class__.__name__} is already installed.")

    def install(self):
        raise NotImplementedError("Subclasses should implement this method")


class NpmInstaller(Installer):
    def is_installed(self):
        try:
            subprocess.check_output(["npm", "--version"])
            return True
        except subprocess.CalledProcessError:
            return False

    def install(self):
        print("Installing npm...")
        subprocess.check_call(["npm", "install", "-g", "npm"])


class PnpmInstaller(Installer):
    def is_installed(self):
        try:
            subprocess.check_output(["pnpm", "--version"])
            return True
        except subprocess.CalledProcessError:
            return False

    def install(self):
        print("Installing pnpm...")
        subprocess.check_call(["npm", "install", "-g", "pnpm"])


class TauriInstaller(Installer):
    def is_installed(self):
        try:
            subprocess.check_output(["cargo", "install", "--list", "tauri-cli"])
            return True
        except subprocess.CalledProcessError:
            return False

    def install(self):
        print("Installing Tauri...")
        subprocess.check_call(["cargo", "install", "tauri-cli"])


def main():
    installers = [NpmInstaller(), PnpmInstaller(), TauriInstaller()]
    for installer in installers:
        installer.prepare_rsc()
        installer.install()


if __name__ == '__main__':
    main()

