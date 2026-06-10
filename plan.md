# Pre commit for delft fews

Delft FEWS configuration is for experts in operational water management.

Precommit is a tool which runs in an ide together with git such as lint.

Make a precommit for xml code which is able to check for mistakes and remove this.

## architecture

write in rust, should be runable as a precommit without any knowlgde of rust. User should be able to easily configure in the .pre-commit.config

## goal

based on precommit should be able to scan xml files and revert those to the default to avoid debugging or test xmls being checked in.

## example

in SystemConfigFiles/Explorer.xml the tag should always be:

```xml
<adjustSystemTimeAutomatically>True</adjustSystemTimeAutomatically>
```

but for debugging this is easier set to 

```xml
<adjustSystemTimeAutomatically>False</adjustSystemTimeAutomatically>
```