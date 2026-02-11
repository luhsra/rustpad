import { Box, Button, Dialog, Flex, Icon, NativeSelect, Popover, Portal } from "@chakra-ui/react";
import { VscOrganization } from "react-icons/vsc";
import { UserInfo } from "./rustpad";
import UserMe, { User } from "./User";


import languages from "./languages.json";


export type FooterProps = {
  language: string;
  currentUser: UserInfo;
  users: Record<number, UserInfo>;
  onLanguageChange: (language: string) => void;
  onLoadSample: () => void;
  onChangeName: (name: string) => void;
  onChangeColor: () => void;
};

function Footer({
  language,
  currentUser,
  users,
  onLanguageChange,
  onLoadSample,
  onChangeName,
  onChangeColor,
}: FooterProps) {
  return (
    <Flex bgColor="#0071c3" color="white" gap={2}>
      <Box>
        <NativeSelect.Root size="xs">
          <NativeSelect.Field value={language} onChange={(event) => onLanguageChange(event.target.value)}>
            {languages.map((lang) => (
              <option key={lang} value={lang} style={{ color: "black" }}>
                {lang}
              </option>
            ))}
          </NativeSelect.Field>
          <NativeSelect.Indicator />
        </NativeSelect.Root>
      </Box>

      <Dialog.Root>
        <Dialog.Trigger>
          <Button variant="outline" size="xs">Sample</Button>
        </Dialog.Trigger>
        <Portal>
          <Dialog.Backdrop />
          <Dialog.Positioner>
            <Dialog.Content>
              <Dialog.CloseTrigger />
              <Dialog.Header>
                <Dialog.Title>Load Sample</Dialog.Title>
              </Dialog.Header>
              <Dialog.Body>
                Delete this document and load the sample?
              </Dialog.Body>
              <Dialog.Footer>
                <Dialog.ActionTrigger asChild>
                  <Button variant="outline">Cancel</Button>
                </Dialog.ActionTrigger>
                <Dialog.ActionTrigger asChild>
                  <Button onClick={onLoadSample}>Load Sample</Button>
                </Dialog.ActionTrigger>
              </Dialog.Footer>
            </Dialog.Content>
          </Dialog.Positioner>
        </Portal>
      </Dialog.Root>

      <Box flex={1}></Box>

      <Popover.Root>
        <Popover.Trigger asChild>
          <Button size="xs" variant="outline">
            <Icon as={VscOrganization} /> {Object.keys(users).length} online
          </Button>
        </Popover.Trigger>
        <Portal>
          <Popover.Positioner>
            <Popover.Content>
              <Popover.Arrow />
              <Popover.Body>
                <Popover.Title fontWeight="semibold">
                  Online Users
                </Popover.Title>
                {Object.entries(users).map(([id, user]) => (
                  <Box key={id}><User info={user} /></Box>
                ))}
              </Popover.Body>
            </Popover.Content>
          </Popover.Positioner>
        </Portal>
      </Popover.Root>

      <UserMe
        info={currentUser}
        onChangeName={onChangeName}
        onChangeColor={onChangeColor}
      />
    </Flex>
  );
}

export default Footer;
