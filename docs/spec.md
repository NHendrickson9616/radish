# Structural plan for Radishes

## UX

### CLI Options
These modify how radishes runs
- `-v`, `-vv`, etc. or `--verbosity {num}` : Runs the subcommand with a specified verbosity (default is none)
- `-t` or `-test`: simply prints out the results of the subcommand instead of running them

### Subcommands
These will be the base subcommands that ship with the binary, but they should be extendable via plugins for things such as `bad` and `play`
 - `import`: Registers files in the database. Depending on config it can move or replace or write files as well. Should also be able to import and convert a beets database.
```bash
radish import ./Music
radish import song_1.mp3 song_2.flac
```
 
 - `config`: Interacts with config file.
    - `-e` or `--edit` edits with default/configured editor
    - `-f` or `--file` outputs the config file path
```bash
radish config -e
radish config -f
```
 
 - `info`: Provides metadata/stats about a file or files. Can specify headers to extract
 - `help`: Provides information about using radishes and subcommands
 - `mv` or `move`: Moves the file(s) and updates their path in the database
 - `remove`: Removes the file(s) from the database
    - `-d` or `--delete`: Deletes the file(s) as well
 - `modify`: Updates the file(s) with the information that follows in key:value format
 - `version`: Outputs the version
 - `write`: Writes info in database onto files
 - `update`: updates the database with the metadata in the files
 - `export`: Exports database to `beets` format


 ### Plugin Roadmap
 These are plugins that will be implemented but have not been yet. Plugins typically have their own subcommands or can be integrated with other subcommands

 - `play`: Plays the file(s) or playlist or directory. Maybe integrate with strawberry (raspberry)?
 - `bad`: Detects if files are badly encoded, can take in custom scripts to run
 - `crdt`: Provides an ordered history of the db modifications so that operations can be undone.
 - `backups`: Backs up database based on config. Can also back up files or portions of library.
 - `ear`: Encryption at rest plugin
 - `garden`: Decentralized sync. Not sure how to implement meeting coordination. Maybe via a torrenting network like soulseek?
 - `farmer`: Centralized sync. Requires a designated central server to function. Central will be SSOT unless otherwise configured, more to be seen.
 - `playlist`: Manages playlists in configured format. Not sure how I would update them as data changes in the db.
 - `rockbox`: Syncs with connected rockbox device.
 - `convert`: Converts between different filetypes
 - `autotag`: Autotags imports or selected file(s) from MusicBrainz API
 - `hello_world`: Test plugin. Prints "Hello, world" to ensure that plugins work


 ## Systems Plan:

 ### Database
 The current plan is to use sqlite for the central db that stores the quintessential structure. Would prefer to keep to one db for simplicity. Headers will be initially derived from beets' current setup and MusicBrainz fields

 #### *\[TODO\]* Information Shape
  Future plans around the shape of the information in the database will be put here

### Constraints
- Importing will need to be threaded. Will also need to be concurrent for the case of autotagging. Maybe it would be best to make it threaded in the main src and then add concurrency only in the autotagger.
- Needs to be able to run non-interactively to integrate with a future gui or other projects.
- Needs to be able to test-run/dry-run
- Code should be written in a way that is easy to logic about, easy to modify to encourage contributors
- TDD
- Minium dependencies for the base binary, plugins will introduce dependencies, but are there to provide a structure to do so by definition.
- Needs to be able to expose information via API instead of printing to integrate with other programs. Perhaps other information vectors?
- Operations need to be CRDT compatible to permit reversibility and syncing


## TODO
- Remove clap dependency / implement built-in arg parser
