using System;
using System.Diagnostics;
using System.IO;
using System.Windows.Forms;

internal static class Multiplayer
{
    [STAThread]
    private static void Main()
    {
        try
        {
            string root = AppDomain.CurrentDomain.BaseDirectory;
            string script = Path.Combine(root, "Community-Multiplayer.ps1");
            if (!File.Exists(script)) throw new FileNotFoundException("Extract the whole multiplayer ZIP before opening Multiplayer.");
            string logs = Path.Combine(root, "iw4l-artifacts", "launcher");
            Directory.CreateDirectory(logs);
            string log = Path.Combine(logs, "startup-" + DateTime.Now.ToString("yyyyMMdd-HHmmss") + ".log");
            ProcessStartInfo start = new ProcessStartInfo
            {
                FileName = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), @"WindowsPowerShell\v1.0\powershell.exe"),
                Arguments = "-NoProfile -STA -ExecutionPolicy Bypass -File \"" + script + "\"",
                WorkingDirectory = root,
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true
            };
            start.EnvironmentVariables["PSModulePath"] = Path.Combine(Path.GetDirectoryName(start.FileName), "Modules")
                + ";" + Environment.GetEnvironmentVariable("PSModulePath");
            using (Process child = Process.Start(start))
            using (StreamWriter writer = new StreamWriter(log))
            {
                object gate = new object();
                DataReceivedEventHandler capture = delegate(object sender, DataReceivedEventArgs data)
                {
                    if (data.Data == null) return;
                    lock (gate) { writer.WriteLine(data.Data); writer.Flush(); }
                };
                child.OutputDataReceived += capture;
                child.ErrorDataReceived += capture;
                child.BeginOutputReadLine();
                child.BeginErrorReadLine();
                child.WaitForExit();
                if (child.ExitCode != 0)
                    MessageBox.Show("The launcher could not start. Details are saved in:\n" + log,
                        "IW4L Multiplayer", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }
        catch (Exception error)
        {
            MessageBox.Show(error.Message, "IW4L Multiplayer", MessageBoxButtons.OK, MessageBoxIcon.Warning);
        }
    }
}
